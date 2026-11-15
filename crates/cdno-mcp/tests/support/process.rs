//! Waiting on a child process that is expected to exit soon, without
//! letting a regression that keeps it running hang the suite.

use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// Run `cmd` to its exit, which the caller expects to come soon, and
/// return its status and stderr. Bounded: a process still running after
/// 15s is killed and the test fails, so a regression that lets it start
/// serving can never hang the suite.
pub fn exit_within_deadline(cmd: &mut Command) -> (ExitStatus, String) {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    // Drained on its own thread so a chatty child cannot block on a full pipe.
    let mut pipe = child.stderr.take().expect("piped stderr");
    let reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = std::io::Read::read_to_string(&mut pipe, &mut buf);
        buf
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the process was still running after 15s; it should have refused at startup");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    (status, reader.join().expect("stderr reader"))
}
