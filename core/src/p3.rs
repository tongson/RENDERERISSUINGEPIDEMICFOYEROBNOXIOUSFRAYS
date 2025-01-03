use {
    crate::packet_bundle::PacketBundle,
    crossbeam_channel::TrySendError,
    paladin_lockup_program::state::LockupPool,
    solana_perf::packet::PacketBatch,
    solana_poh::poh_recorder::PohRecorder,
    solana_sdk::{
        account::ReadableAccount,
        packet::{Packet, PACKET_DATA_SIZE},
        pubkey::Pubkey,
        saturating_add_assign,
        transaction::VersionedTransaction,
    },
    spl_discriminator::discriminator::SplDiscriminate,
    std::{
        collections::HashMap,
        net::{SocketAddr, UdpSocket},
        ops::AddAssign,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        time::{Duration, Instant},
    },
};

pub const P3_SOCKET_DEFAULT: &str = "0.0.0.0:4818";

const READ_TIMEOUT: Duration = Duration::from_millis(100);
const RATE_LIMIT_UPDATE_INTERVAL: Duration = Duration::from_secs(300); // 5 minutes
const PACKETS_PER_SECOND: u64 = 5_000;
const POOL_KEY: Pubkey = solana_sdk::pubkey!("EJi4Rj2u1VXiLpKtaqeQh3w4XxAGLFqnAG1jCorSvVmg");

pub(crate) struct P3 {
    exit: Arc<AtomicBool>,

    bundle_stage_tx: crossbeam_channel::Sender<Vec<PacketBundle>>,

    socket: UdpSocket,
    buffer: [u8; PACKET_DATA_SIZE],
    rate_limits: HashMap<Pubkey, RateLimit>,
    rate_limits_last_update: Instant,

    metrics: P3Metrics,
    metrics_creation: Instant,
    poh_recorder: Arc<RwLock<PohRecorder>>,
}

impl P3 {
    pub(crate) fn spawn(
        exit: Arc<AtomicBool>,
        bundle_stage_tx: crossbeam_channel::Sender<Vec<PacketBundle>>,
        addr: SocketAddr,
        poh_recorder: Arc<RwLock<PohRecorder>>,
    ) -> std::thread::JoinHandle<()> {
        let socket = UdpSocket::bind(addr).unwrap();
        socket.set_read_timeout(Some(READ_TIMEOUT)).unwrap();

        let p3 = Self {
            exit: exit.clone(),
            bundle_stage_tx,
            socket,
            buffer: [0u8; PACKET_DATA_SIZE],
            rate_limits: HashMap::default(),
            rate_limits_last_update: Instant::now(),

            metrics: P3Metrics::default(),
            metrics_creation: Instant::now(),
            poh_recorder: poh_recorder.clone(),
        };

        std::thread::Builder::new()
            .name("P3".to_owned())
            .spawn(move || p3.run())
            .unwrap()
    }

    fn run(mut self) {
        self.update_rate_limits();

        while !self.exit.load(Ordering::Relaxed) {
            // Try receive packets.
            let (tx, _) = match self.socket_recv() {
                Some(Ok(result)) => result,
                Some(Err(_)) => continue,
                None => {
                    // NB: Intentionally only check to update rate limits when socket is empty.
                    if self.rate_limits_last_update.elapsed() >= RATE_LIMIT_UPDATE_INTERVAL {
                        self.update_rate_limits();
                        self.rate_limits_last_update = Instant::now();
                        trace!("Update rate limits; rate_limits={:?}", self.rate_limits);
                    }

                    continue;
                }
            };

            // Check if we need to report metrics for the last interval.
            let now = Instant::now();
            if now - self.metrics_creation > Duration::from_secs(1) {
                self.metrics.report();
                self.metrics = P3Metrics::default();
                self.metrics_creation = now;
            }

            let Some(signature) = tx.signatures.get(0) else {
                warn!("TX received without signature");
                continue;
            };
            trace!("Received TX; signature={signature}");

            let packet_bundle = PacketBundle {
                batch: PacketBatch::new(vec![Packet::from_data(None, &tx).unwrap()]),
                bundle_id: format!("R{signature}"),
            };

            match self.bundle_stage_tx.try_send(vec![packet_bundle]) {
                Ok(_) => {}
                Err(TrySendError::Disconnected(_)) => break,
                Err(TrySendError::Full(_)) => {
                    warn!("Dropping TX; signature={}", signature);
                    saturating_add_assign!(self.metrics.dropped, 1);
                }
            }
        }
    }

    fn socket_recv(&mut self) -> Option<Result<(VersionedTransaction, SocketAddr), ()>> {
        match self.socket.recv_from(&mut self.buffer) {
            Ok((_, src_addr)) => {
                self.metrics.transactions.add_assign(1);

                Some(
                    bincode::deserialize::<VersionedTransaction>(&self.buffer)
                        .inspect_err(|_| saturating_add_assign!(self.metrics.err_deserialize, 1))
                        .map(|tx| (tx, src_addr))
                        .map_err(|_| ()),
                )
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => None,
            Err(err) => {
                error!("Unexpected IO error; err={err}");

                // NB: Return None here as we are unsure if the socket has more
                // packets and do not want to infinite loop.
                None
            }
        }
    }

    fn update_rate_limits(&mut self) {
        let bank = self.poh_recorder.read().unwrap().latest_bank();

        // Load the lockup pool account.
        let Some(pool) = bank.get_account(&POOL_KEY) else {
            warn!("Lockup pool does not exist; pool={POOL_KEY}");

            return;
        };

        // Try to deserialize the pool.
        let Some(pool) = Self::try_deserialize_lockup_pool(pool.data()) else {
            warn!("Failed to deserialize lockup pool; pool={POOL_KEY}");

            return;
        };

        // Compute the new total locked PAL.
        let entries = pool
            .entries
            .iter()
            .take_while(|entry| entry.lockup != Pubkey::default());
        let total_pal = entries.clone().map(|entry| entry.amount).sum();

        // Clear the old entries & write the new ones.
        self.rate_limits.clear();
        self.rate_limits.extend(entries.clone().map(|entry| {
            let cap = Self::compute_cap(entry.amount, total_pal);

            (
                Pubkey::new_from_array(entry.metadata),
                RateLimit {
                    cap,
                    remaining: cap,
                    last: Instant::now(),
                },
            )
        }));
    }

    fn try_deserialize_lockup_pool(data: &[u8]) -> Option<&LockupPool> {
        if data.len() < 8 || &data[0..8] != LockupPool::SPL_DISCRIMINATOR.as_slice() {
            return None;
        }

        bytemuck::try_from_bytes::<LockupPool>(data).ok()
    }

    fn compute_cap(amount: u64, total: u64) -> u64 {
        amount
            .saturating_mul(PACKETS_PER_SECOND)
            .checked_div(total)
            .unwrap_or_else(|| {
                println!("ERR: Total == 0 but compute_cap was called");

                0
            })
    }
}

#[allow(dead_code)]
#[derive(Debug)]
struct RateLimit {
    cap: u64,
    remaining: u64,
    last: Instant,
}

#[derive(Default, PartialEq, Eq)]
struct P3Metrics {
    /// Number of transactions received.
    transactions: u64,
    /// Number of transactions dropped due to full channel.
    dropped: u64,
    /// Number of transactions that failed to deserialize.
    err_deserialize: u64,
}

impl P3Metrics {
    fn report(&self) {
        // Suppress logs if there are no recorded metrics.
        if self == &P3Metrics::default() {
            return;
        }

        datapoint_info!(
            "p3_socket",
            ("transactions", self.transactions as i64, i64),
            ("dropped", self.dropped as i64, i64),
            ("err_deserialize", self.err_deserialize as i64, i64)
        );
    }
}
