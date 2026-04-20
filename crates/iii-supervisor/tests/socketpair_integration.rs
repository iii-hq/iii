// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! End-to-end test of `control::serve` over a real unix socket pair.
//!
//! Stands in for the virtio-console port: in production the supervisor
//! reads requests from `/dev/vport0p1`, which libkrun backs with one
//! end of a host-provided fd pair. A plain `socketpair(AF_UNIX, STREAM)`
//! has identical semantics for our purposes — bidirectional, lossless,
//! orderly stream of bytes. If serve works over this, it works over the
//! real transport.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::thread;
use std::time::Duration;

use iii_supervisor::child::{Config, State};
use iii_supervisor::control;
use iii_supervisor::protocol::{self, Request, Response};

fn send(stream: &mut UnixStream, req: &Request) -> Response {
    let line = protocol::encode_request(req);
    writeln!(stream, "{line}").unwrap();
    stream.flush().unwrap();
    let mut reader = BufReader::new(stream);
    let mut resp_line = String::new();
    reader.read_line(&mut resp_line).unwrap();
    protocol::decode_response(&resp_line).unwrap()
}

#[test]
fn restart_cycle_over_socketpair() {
    let (host, guest) = UnixStream::pair().unwrap();

    // Supervisor runs in a background thread, reading/writing `guest`.
    let guest_read = guest.try_clone().unwrap();
    let guest_write = guest;
    let state = State::new(Config {
        run_cmd: "sleep 30".to_string(),
        workdir: "/tmp".to_string(),
    });
    state.spawn_initial().unwrap();
    let state_handle = state.clone();

    let server = thread::spawn(move || {
        control::serve(state_handle, BufReader::new(guest_read), guest_write).unwrap();
    });

    let mut host = host;

    // Ping → Alive with the current pid
    let resp = send(&mut host, &Request::Ping);
    let initial_pid = match resp {
        Response::Alive { pid } => {
            assert!(pid > 0);
            pid
        }
        other => panic!("expected Alive, got {other:?}"),
    };

    // Restart → Ok, and the pid rotates
    let resp = send(&mut host, &Request::Restart);
    assert_eq!(resp, Response::Ok);

    let resp = send(&mut host, &Request::Status);
    match resp {
        Response::Status { pid, restarts } => {
            assert_eq!(restarts, 1);
            assert!(pid.is_some());
            assert_ne!(pid, Some(initial_pid), "pid must rotate");
        }
        other => panic!("expected Status, got {other:?}"),
    }

    // Shutdown → Ok, server loop exits
    let resp = send(&mut host, &Request::Shutdown);
    assert_eq!(resp, Response::Ok);

    // Drop the host end so the serve loop sees EOF if it hasn't already
    // exited via the shutdown path.
    drop(host);

    let joined = server.join();
    assert!(joined.is_ok(), "server thread panicked: {joined:?}");

    // Child is dead.
    assert_eq!(state.pid(), None);
}

#[test]
fn multiple_pings_do_not_kill_the_child() {
    let (host, guest) = UnixStream::pair().unwrap();
    let guest_read = guest.try_clone().unwrap();
    let state = State::new(Config {
        run_cmd: "sleep 30".to_string(),
        workdir: "/tmp".to_string(),
    });
    state.spawn_initial().unwrap();

    let state_server = state.clone();
    let server = thread::spawn(move || {
        control::serve(state_server, BufReader::new(guest_read), guest).unwrap();
    });

    let mut host = host;
    let mut pids = Vec::new();
    for _ in 0..5 {
        match send(&mut host, &Request::Ping) {
            Response::Alive { pid } => pids.push(pid),
            other => panic!("{other:?}"),
        }
    }
    assert!(pids.windows(2).all(|w| w[0] == w[1]), "pid must not change");
    assert_eq!(state.restarts(), 0, "ping must not restart");

    send(&mut host, &Request::Shutdown);
    drop(host);
    let _ = server.join();
}

#[test]
fn malformed_request_does_not_kill_the_channel() {
    let (mut host, guest) = UnixStream::pair().unwrap();
    let guest_read = guest.try_clone().unwrap();
    let state = State::new(Config {
        run_cmd: "sleep 30".to_string(),
        workdir: "/tmp".to_string(),
    });
    state.spawn_initial().unwrap();

    let state_server = state.clone();
    let server = thread::spawn(move || {
        control::serve(state_server, BufReader::new(guest_read), guest).unwrap();
    });

    // Send garbage.
    writeln!(host, "not a json").unwrap();
    host.flush().unwrap();
    let mut reader = BufReader::new(host.try_clone().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let resp = protocol::decode_response(&line).unwrap();
    assert!(matches!(resp, Response::Error { .. }));

    // Channel is still live — a subsequent valid request works.
    let resp = send(&mut host, &Request::Ping);
    assert!(matches!(resp, Response::Alive { .. }));

    send(&mut host, &Request::Shutdown);
    drop(host);
    let _ = server.join();
}

#[test]
fn eof_closes_loop_cleanly() {
    let (host, guest) = UnixStream::pair().unwrap();
    let guest_read = guest.try_clone().unwrap();
    let state = State::new(Config {
        run_cmd: "sleep 30".to_string(),
        workdir: "/tmp".to_string(),
    });
    state.spawn_initial().unwrap();

    let state_server = state.clone();
    let server = thread::spawn(move || {
        control::serve(state_server, BufReader::new(guest_read), guest).unwrap();
    });

    // Close the host end without any requests.
    drop(host);

    // Serve loop should observe EOF and return Ok.
    let joined = server.join();
    assert!(joined.is_ok());

    // The child is still alive (shutdown wasn't requested), clean up.
    state.kill_for_shutdown().unwrap();

    // Small sleep so the OS reaps the child's exit status before the
    // test harness's global teardown.
    thread::sleep(Duration::from_millis(50));
}
