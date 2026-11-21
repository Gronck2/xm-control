//! The device worker (ADR 0004): one thread owns the `Session`, runs the handshake and the initial sync, executes `Command`s from a channel,
//! turns incoming RET/NTFY frames into `DeviceState` updates and reports everything as `Event`s. UI-neutral: the UI reads events from the
//! receiver (the optional wake callback says "something is waiting", e.g. to post a window message).

pub mod battery;
pub mod dsee;
pub mod eq;
pub mod feature;
pub mod handshake;
#[cfg(test)]
pub mod mock;
pub mod ncasm;
pub mod peripheral;
pub mod power;
pub mod state;
pub mod stc;
pub mod wearing;

use crate::bt::{MDR_V2_UUID, parse_guid, winsock::WinsockRfcomm};
use crate::mdr::frame::{Frame, FrameError, TYPE_T1};
use crate::mdr::guard::{GuardError, Mode, Table, check};
use crate::mdr::session::{Session, SessionError, Timeouts, Transport};
use feature::{Feature, Get, Parsed, supported};
use handshake::{DeviceInfo, handshake};
use state::DeviceState;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

/// All features, in sync order. Feature tasks add one line each.
pub static FEATURES: &[&dyn Feature] = &[
    &battery::BatteryFeature,
    &ncasm::NcAsmFeature,
    &eq::EqFeature,
    &dsee::DseeFeature,
    &stc::StcFeature,
    &power::PowerOffFeature,
    &power::AutoPauseFeature,
    &wearing::WearingFeature,
    &peripheral::MultipointFeature,
    &peripheral::DevicesFeature,
];

/// How long the worker waits for incoming frames between checks of the command channel.
/// shortcut: a command can wait up to this long before it is sent; fine for button-driven UIs, use a wakeable transport read if it ever matters.
const POLL: Duration = Duration::from_millis(50);
/// Fallback re-read interval for features that ask for it (`Feature::wants_refresh`).
const REFRESH: Duration = Duration::from_secs(60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

pub type Wake = Box<dyn Fn() + Send>;

#[derive(Debug)]
#[allow(dead_code)] // fields are used by the UI and tests; the dev CLI only prints them
pub enum Command {
    /// Re-read a feature (its initial GETs).
    Get { feature: String },
    /// Set a feature from user text; the feature encodes it and the guard approves it.
    Set { feature: String, value: String },
}

#[derive(Debug)]
#[allow(dead_code)] // fields are used by the UI and tests; the dev CLI only prints them
pub enum CommandError {
    UnknownFeature(String),
    /// The headset does not advertise the feature; nothing was sent.
    NotSupported(String),
    BadValue(String),
    Guard(GuardError),
    Frame(FrameError),
}

#[derive(Debug)]
#[allow(dead_code)] // fields are used by the UI and tests; the dev CLI only prints them
pub enum Event {
    Connected(DeviceInfo),
    /// The initial GETs are done (answered or timed out); the snapshot is the state at that point.
    Synced(DeviceState),
    /// One feature changed (a reply or a headset-side change); the snapshot is the whole state after the change.
    StateChanged {
        feature: &'static str,
        state: DeviceState,
    },
    /// A well-formed frame no feature understood (logged by the dev CLI).
    Unhandled {
        table: Table,
        payload: Vec<u8>,
    },
    /// A command was refused or could not be encoded; the session goes on.
    Error(CommandError),
    /// The session ended (link lost, connect failed, or the handle was dropped); the worker thread exits and the event channel closes.
    Disconnected(String),
}

/// The UI side. Dropping it stops the worker, but only once the current handshake, initial sync or command finishes (can take
/// seconds), then within about `POLL`; the thread is not joined and the RFCOMM socket stays open until then. A UI must wait
/// for the event channel to close before reconnecting.
pub struct DeviceHandle {
    commands: Sender<Command>,
}

impl DeviceHandle {
    /// Connects to the headset at `mac` on the worker thread (events: `Connected` or `Disconnected`). No automatic reconnect: after
    /// `Disconnected` the UI connects again with a new handle.
    pub fn connect(mac: u64, wake: Option<Wake>) -> (Self, Receiver<Event>) {
        let open = move || {
            let uuid = parse_guid(MDR_V2_UUID).expect("constant UUID is valid");
            WinsockRfcomm::connect(mac, uuid, CONNECT_TIMEOUT).map_err(|e| format!("connect failed: {e}"))
        };
        Self::spawn(open, FEATURES, Timeouts::default(), REFRESH, wake)
    }

    /// `open` runs on the worker thread, so the transport itself need not be `Send` and a slow connect does not block the caller.
    pub fn spawn<T: Transport + 'static>(
        open: impl FnOnce() -> Result<T, String> + Send + 'static,
        features: &'static [&'static dyn Feature],
        timeouts: Timeouts,
        refresh: Duration,
        wake: Option<Wake>,
    ) -> (Self, Receiver<Event>) {
        let (commands, command_rx) = mpsc::channel();
        let (event_tx, events) = mpsc::channel();
        std::thread::spawn(move || {
            let mut w = Worker { features, events: event_tx, wake, refresh, state: DeviceState::default() };
            let reason = match w.run(open, timeouts, command_rx) {
                Err(Stop::Lost(reason)) => reason,
                Err(Stop::Gone) => return,
                Ok(()) => "closed".to_string(),
            };
            let _ = w.emit(Event::Disconnected(reason));
        });
        (DeviceHandle { commands }, events)
    }

    /// Queues a command; commands run one at a time in the order sent, whichever thread sends them. Errors if the worker has ended.
    pub fn send(&self, command: Command) -> Result<(), mpsc::SendError<Command>> {
        self.commands.send(command)
    }
}

enum Stop {
    /// Nobody listens to the events any more.
    Gone,
    Lost(String),
}

/// Why one command or GET did not complete.
enum Fail {
    Command(CommandError),
    Stop(Stop),
}

impl From<CommandError> for Fail {
    fn from(e: CommandError) -> Self {
        Fail::Command(e)
    }
}

impl From<Stop> for Fail {
    fn from(e: Stop) -> Self {
        Fail::Stop(e)
    }
}

impl From<SessionError> for Fail {
    fn from(e: SessionError) -> Self {
        match e {
            SessionError::Frame(e) => Fail::Command(CommandError::Frame(e)),
            e => Fail::Stop(Stop::Lost(e.to_string())),
        }
    }
}

struct Worker {
    features: &'static [&'static dyn Feature],
    events: Sender<Event>,
    wake: Option<Wake>,
    refresh: Duration,
    state: DeviceState,
}

impl Worker {
    fn emit(&self, e: Event) -> Result<(), Stop> {
        self.events.send(e).map_err(|_| Stop::Gone)?;
        if let Some(w) = &self.wake {
            w();
        }
        Ok(())
    }

    fn run<T: Transport>(&mut self, open: impl FnOnce() -> Result<T, String>, timeouts: Timeouts, commands: Receiver<Command>) -> Result<(), Stop> {
        let mut s = Session::new(open().map_err(Stop::Lost)?, timeouts);
        let r = self.serve(&mut s, commands);
        s.close();
        r
    }

    fn serve<T: Transport>(&mut self, s: &mut Session<T>, commands: Receiver<Command>) -> Result<(), Stop> {
        let info = handshake(s).map_err(|e| Stop::Lost(e.to_string()))?;
        self.emit(Event::Connected(info.clone()))?;
        for f in self.features.iter().filter(|f| supported(**f, &info)) {
            for g in f.initial_gets(&info) {
                let r = self.fetch(s, &g);
                self.finish(r)?;
            }
        }
        self.emit(Event::Synced(self.state.clone()))?;
        let mut last = Instant::now();
        loop {
            if last.elapsed() >= self.refresh {
                last = Instant::now();
                let features = self.features;
                let due: Vec<_> = features.iter().filter(|f| supported(**f, &info) && f.wants_refresh(&self.state)).collect();
                for f in due {
                    for g in f.initial_gets(&info) {
                        let r = self.fetch(s, &g);
                        self.finish(r)?;
                    }
                }
            }
            match commands.try_recv() {
                Ok(c) => {
                    let r = self.execute(s, &info, c);
                    self.finish(r)?;
                    continue;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => return Ok(()),
            }
            let frames = s.poll_notifications(POLL).map_err(|e| Stop::Lost(e.to_string()))?;
            self.dispatch(frames)?;
        }
    }

    /// A refused command is reported and the session goes on; anything else ends it.
    fn finish(&self, r: Result<(), Fail>) -> Result<(), Stop> {
        match r {
            Ok(()) => Ok(()),
            Err(Fail::Command(e)) => self.emit(Event::Error(e)),
            Err(Fail::Stop(s)) => Err(s),
        }
    }

    fn execute<T: Transport>(&mut self, s: &mut Session<T>, info: &DeviceInfo, command: Command) -> Result<(), Fail> {
        let (name, value) = match &command {
            Command::Get { feature } => (feature, None),
            Command::Set { feature, value } => (feature, Some(value)),
        };
        let f = *self.features.iter().find(|f| f.name() == name).ok_or_else(|| CommandError::UnknownFeature(name.clone()))?;
        if !supported(f, info) {
            return Err(CommandError::NotSupported(name.clone()).into());
        }
        match value {
            None => {
                for g in f.initial_gets(info) {
                    self.fetch(s, &g)?;
                }
            }
            Some(v) => {
                let (table, payload) = f.encode_set(&self.state, v).map_err(CommandError::BadValue)?;
                let cmd = check(table, &payload, Mode::Normal).map_err(CommandError::Guard)?;
                s.send(&cmd)?;
                // the new value arrives as a reply or NTFY; the state is not guessed
                let frames = s.poll_notifications(Duration::ZERO)?;
                self.dispatch(frames)?;
                for g in f.after_set(info) {
                    self.fetch(s, &g)?;
                }
            }
        }
        Ok(())
    }

    /// One GET. ACKed but unanswered (or answered with another id) is not an error: whatever arrives goes through `dispatch`.
    fn fetch<T: Transport>(&mut self, s: &mut Session<T>, g: &Get) -> Result<(), Fail> {
        let cmd = check(g.table, &g.payload, Mode::ReadOnly).map_err(CommandError::Guard)?;
        match s.request(&cmd, g.reply_id) {
            Ok(reply) => self.dispatch(vec![reply])?,
            Err(SessionError::NoReply) => {}
            Err(e) => return Err(e.into()),
        }
        let frames = s.poll_notifications(Duration::ZERO)?;
        self.dispatch(frames)?;
        Ok(())
    }

    fn dispatch(&mut self, frames: Vec<Frame>) -> Result<(), Stop> {
        for f in frames {
            let table = if f.data_type == TYPE_T1 { Table::T1 } else { Table::T2 };
            let mut known = false;
            for feat in self.features {
                match feat.on_frame(&mut self.state, table, &f.payload) {
                    Parsed::NotMine => {}
                    Parsed::Unchanged => known = true,
                    Parsed::Changed => {
                        known = true;
                        self.emit(Event::StateChanged { feature: feat.name(), state: self.state.clone() })?;
                    }
                }
            }
            if !known {
                self.emit(Event::Unhandled { table, payload: f.payload })?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mdr::frame::{TYPE_ACK, unpack};
    use mock::FakeHeadset;
    use std::sync::{Arc, Mutex};

    const FAST: Timeouts = Timeouts { ack: Duration::from_millis(15), response: Duration::from_millis(60), tries: 3 };
    const WAIT: Duration = Duration::from_secs(5);
    const PING: [u8; 2] = [0x20, 0x04]; // an allowlisted GET the test feature uses as its "SET"
    const DIE: [u8; 2] = [0x26, 0x04]; // not in the capture: the test headset drops the link when it is written

    /// GETs the battery (answered), the EQ status (ACKed, never answered) and safe listening HBS_1 (T2 `5A 00`, answered with `5B 02`).
    struct Dummy;
    impl Feature for Dummy {
        fn name(&self) -> &'static str {
            "dummy"
        }
        fn required(&self) -> &'static [(Table, u8)] {
            &[(Table::T1, 0x20)]
        }
        fn initial_gets(&self, _: &DeviceInfo) -> Vec<Get> {
            vec![
                Get { table: Table::T1, payload: vec![0x22, 0x00], reply_id: 0x23 },
                Get { table: Table::T1, payload: vec![0x52, 0x00], reply_id: 0x53 },
                Get { table: Table::T2, payload: vec![0x5A, 0x00], reply_id: 0x5B },
            ]
        }
        fn on_frame(&self, state: &mut DeviceState, table: Table, payload: &[u8]) -> Parsed {
            let ((Table::T1, [0x23, _, rest @ ..]) | (Table::T2, [0x5B, _, rest @ ..])) = (table, payload) else { return Parsed::NotMine };
            if state.dummy.as_deref() == Some(rest) {
                return Parsed::Unchanged;
            }
            state.dummy = Some(rest.to_vec());
            Parsed::Changed
        }
        fn encode_set(&self, _: &DeviceState, value: &str) -> Result<(Table, Vec<u8>), String> {
            match value {
                "ping" => Ok((Table::T1, PING.to_vec())),
                "die" => Ok((Table::T1, DIE.to_vec())),
                "bad" => Ok((Table::T1, vec![0x30, 0x00])), // firmware update: the guard must refuse it
                _ => Err(format!("bad value {value:?}")),
            }
        }
    }

    /// Needs T1 0x24, which the XM6 does not advertise (section 8.1).
    struct Absent;
    impl Feature for Absent {
        fn name(&self) -> &'static str {
            "absent"
        }
        fn required(&self) -> &'static [(Table, u8)] {
            &[(Table::T1, 0x24)]
        }
        fn initial_gets(&self, _: &DeviceInfo) -> Vec<Get> {
            panic!("an unsupported feature must not sync")
        }
        fn on_frame(&self, _: &mut DeviceState, _: Table, _: &[u8]) -> Parsed {
            Parsed::NotMine
        }
        fn encode_set(&self, _: &DeviceState, _: &str) -> Result<(Table, Vec<u8>), String> {
            Ok((Table::T1, vec![0x22, 0x00]))
        }
    }

    static TEST_FEATURES: &[&dyn Feature] = &[&Dummy, &Absent];

    type Writes = Arc<Mutex<Vec<Vec<u8>>>>;

    fn capture() -> FakeHeadset {
        let mut h = FakeHeadset::from_xm6_capture();
        h.no_reply.push(PING.to_vec()); // ACK only, so a scripted NTFY has something to follow
        h
    }

    fn start(h: FakeHeadset) -> (DeviceHandle, Receiver<Event>, Writes) {
        let writes = h.writes.clone();
        let (handle, rx) = DeviceHandle::spawn(move || Ok(h), TEST_FEATURES, FAST, REFRESH, None);
        (handle, rx, writes)
    }

    fn next(rx: &Receiver<Event>) -> Event {
        rx.recv_timeout(WAIT).expect("an event")
    }

    /// Events up to and including `Synced`.
    fn sync(rx: &Receiver<Event>) -> Vec<Event> {
        let mut v = Vec::new();
        loop {
            let e = next(rx);
            let done = matches!(e, Event::Synced(_));
            v.push(e);
            if done {
                return v;
            }
        }
    }

    fn quiet(rx: &Receiver<Event>) {
        assert!(matches!(rx.recv_timeout(Duration::from_millis(200)), Err(mpsc::RecvTimeoutError::Timeout)));
    }

    fn sent(w: &Writes) -> Vec<(u8, Vec<u8>, u8)> {
        w.lock().unwrap().iter().filter_map(|b| unpack(b).ok()).filter(|f| f.data_type != TYPE_ACK).map(|f| (f.data_type, f.payload, f.seq)).collect()
    }

    fn set(h: &DeviceHandle, value: &str) {
        h.send(Command::Set { feature: "dummy".into(), value: value.into() }).unwrap();
    }

    #[test]
    fn initial_sync_on_the_real_capture() {
        let (h, rx, w) = start(capture());
        let events = sync(&rx);
        let Event::Connected(info) = &events[0] else { panic!("{events:?}") };
        assert_eq!((info.model_name.as_str(), info.firmware.as_str()), ("WH-1000XM6", "3.1.5"));
        assert_eq!((info.t1_functions.len(), info.t2_functions.len()), (45, 16));
        // battery reply, then the late `5B 02` (T2 `5A 00` was answered with another type byte): delivered to the feature, not dropped
        let states: Vec<_> = events.iter().filter_map(|e| if let Event::StateChanged { state, .. } = e { state.dummy.clone() } else { None }).collect();
        assert_eq!(states, vec![vec![0x61, 0x00], vec![0x00, 0x03]]);
        assert!(!events.iter().any(|e| matches!(e, Event::Error(_) | Event::Disconnected(_) | Event::Unhandled { .. })));
        // the ACKed-but-silent `52 00` was asked once (no retry after an ACK) and the session lives on
        let cmds = sent(&w);
        assert_eq!(cmds.iter().filter(|(_, p, _)| *p == [0x52, 0x00]).count(), 1);
        assert!(cmds.iter().any(|(t, p, _)| *t == 0x0E && *p == [0x5A, 0x00]));
        quiet(&rx);
        // not advertised: refused without touching the transport
        let before = w.lock().unwrap().len();
        for c in [Command::Get { feature: "absent".into() }, Command::Set { feature: "absent".into(), value: "x".into() }] {
            h.send(c).unwrap();
            assert!(matches!(next(&rx), Event::Error(CommandError::NotSupported(n)) if n == "absent"));
        }
        h.send(Command::Get { feature: "nope".into() }).unwrap();
        assert!(matches!(next(&rx), Event::Error(CommandError::UnknownFeature(_))));
        assert_eq!(w.lock().unwrap().len(), before);
    }

    #[test]
    fn unsolicited_ntfy_updates_state_with_one_event() {
        let mut h = capture();
        h.ntfy_on = Some((PING.to_vec(), vec![0x23, 0x00, 0x50, 0x00]));
        let (handle, rx, w) = start(h);
        sync(&rx);
        set(&handle, "ping");
        let Event::StateChanged { feature, state } = next(&rx) else { panic!() };
        assert_eq!((feature, state.dummy), ("dummy", Some(vec![0x50, 0x00])));
        quiet(&rx);
        // the NTFY was ACKed
        assert_eq!(unpack(w.lock().unwrap().last().unwrap()).unwrap().data_type, TYPE_ACK);
    }

    #[test]
    fn guard_refusal_and_bad_value_send_nothing() {
        let (h, rx, w) = start(capture());
        sync(&rx);
        let before = w.lock().unwrap().len();
        set(&h, "bad");
        assert!(matches!(next(&rx), Event::Error(CommandError::Guard(GuardError::Denied))));
        set(&h, "???");
        assert!(matches!(next(&rx), Event::Error(CommandError::BadValue(_))));
        assert_eq!(w.lock().unwrap().len(), before);
    }

    #[test]
    fn commands_from_two_threads_do_not_interleave() {
        let mut hs = capture();
        hs.fail_on = Some(DIE.to_vec());
        let (h, rx, w) = start(hs);
        sync(&rx);
        std::thread::scope(|s| {
            for _ in 0..2 {
                s.spawn(|| (0..5).for_each(|_| set(&h, "ping")));
            }
        });
        set(&h, "die"); // its failure ends the session, so everything before it has been processed
        assert!(matches!(next(&rx), Event::Disconnected(_)));
        let pings: Vec<_> = sent(&w).into_iter().skip_while(|(_, p, _)| *p != PING).collect();
        assert_eq!(pings.iter().filter(|(_, p, _)| *p == PING).count(), 10);
        // stop-and-wait: one command frame at a time, sequence 0/1 alternating
        assert!(pings.windows(2).all(|p| p[0].2 != p[1].2));
    }

    #[test]
    fn transport_error_disconnects_and_ends_the_thread() {
        let mut h = capture();
        h.fail_on = Some(DIE.to_vec());
        let (handle, rx, _) = start(h);
        sync(&rx);
        set(&handle, "die");
        assert!(matches!(next(&rx), Event::Disconnected(_)));
        assert!(matches!(rx.recv_timeout(WAIT), Err(mpsc::RecvTimeoutError::Disconnected)));
    }

    #[test]
    fn dropping_the_handle_ends_the_thread() {
        let (handle, rx, _) = start(capture());
        sync(&rx);
        drop(handle);
        assert!(matches!(next(&rx), Event::Disconnected(_)));
        assert!(matches!(rx.recv_timeout(WAIT), Err(mpsc::RecvTimeoutError::Disconnected)));
    }

    #[test]
    fn failed_connect_is_a_disconnect() {
        let (_h, rx) = DeviceHandle::spawn(|| Err::<FakeHeadset, _>("no such device".to_string()), TEST_FEATURES, FAST, REFRESH, None);
        assert!(matches!(next(&rx), Event::Disconnected(r) if r == "no such device"));
    }
}
