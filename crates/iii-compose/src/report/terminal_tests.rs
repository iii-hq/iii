use super::*;

fn progress(workers: usize) -> Console {
    let mut startup = StartupRows::new(true);
    startup.engine.state = RowState::Ready {
        what: "Ready".to_string(),
        elapsed: Duration::from_secs(2),
    };
    startup.downloads.state = RowState::Skipped("No downloads".to_string());
    startup.containers.state = RowState::Starting {
        what: "Starting".to_string(),
        began: Instant::now(),
    };
    Console {
        startup: Some(startup),
        rows: (0..workers)
            .map(|index| Row {
                key: format!("worker-{index:02}"),
                depth: index % 3,
                state: RowState::Starting {
                    what: "waiting for engine registration".to_string(),
                    began: Instant::now(),
                },
            })
            .collect(),
        ..Console::default()
    }
}

fn write_terminal(terminal: &mut vt100::Parser, output: &str) {
    // Match the newline conversion performed by a normal PTY (ONLCR).
    terminal.process(output.replace('\n', "\r\n").as_bytes());
}

fn screen_and_history(terminal: &mut vt100::Parser) -> String {
    let screen = terminal.screen_mut();
    let (_, width) = screen.size();
    screen.set_scrollback(usize::MAX);
    let history = screen.scrollback();
    let mut text = String::new();
    for offset in (1..=history).rev() {
        screen.set_scrollback(offset);
        text.push_str(&screen.rows(0, width).next().unwrap());
        text.push('\n');
    }
    screen.set_scrollback(0);
    text.push_str(&screen.contents());
    text
}

const DOWNLOAD_WORKERS: [(&str, usize); 13] = [
    ("llm-router", 0),
    ("provider-openai", 1),
    ("provider-anthropic", 1),
    ("context-manager", 1),
    ("cron", 0),
    ("harness", 2),
    ("iii-directory", 0),
    ("console", 0),
    ("session-manager", 0),
    ("queue", 0),
    ("web", 0),
    ("state", 0),
    ("shell", 0),
];

#[test]
fn downloaded_workers_keep_one_row_through_every_startup_phase() {
    for (height, width) in [(24, 80), (18, 120)] {
        let mut state = progress(0);
        state.startup.as_mut().unwrap().downloads.state = RowState::Starting {
            what: "Downloading (0/13)".to_string(),
            began: Instant::now(),
        };
        state.startup.as_mut().unwrap().containers.state = RowState::Waiting;
        let mut terminal = vt100::Parser::new(height, width, 1000);
        // Downloads register dynamically, and transfer amounts/rates grow wider
        // as chunks arrive. Real names must fit throughout, not only at 0%.
        for (key, _) in DOWNLOAD_WORKERS {
            state.downloads.push(Row {
                key: key.to_string(),
                depth: 1,
                state: RowState::Downloading {
                    downloaded: 0,
                    total: Some(8 * 1024 * 1024),
                    began: Instant::now() - Duration::from_millis(10),
                },
            });
            write_terminal(&mut terminal, &state.render(Some((height, width))));
        }
        for step in 1..=64 {
            for row in &mut state.downloads {
                if let RowState::Downloading { downloaded, .. } = &mut row.state {
                    *downloaded = step * 16 * 1024;
                }
            }
            write_terminal(&mut terminal, &state.render(Some((height, width))));
            assert!(!state.static_output, "{width}x{height}, chunk {step}");
        }
        for index in 0..13 {
            state.downloads[index].state = RowState::Downloaded {
                downloaded: 1024 * 1024,
                total: Some(1024 * 1024),
                elapsed: Duration::from_secs(1),
            };
            write_terminal(&mut terminal, &state.render(Some((height, width))));
        }
        state.startup.as_mut().unwrap().downloads.state = RowState::Ready {
            what: "Complete (13)".to_string(),
            elapsed: Duration::from_secs(1),
        };
        state.startup.as_mut().unwrap().containers.state = RowState::Starting {
            what: "Starting (0/13)".to_string(),
            began: Instant::now(),
        };
        state.rows = DOWNLOAD_WORKERS
            .iter()
            .map(|(key, depth)| Row {
                key: (*key).to_string(),
                depth: *depth,
                state: RowState::Waiting,
            })
            .collect();
        write_terminal(&mut terminal, &state.render(Some((height, width))));
        let text = screen_and_history(&mut terminal);
        assert!(!state.static_output, "{width}x{height}: {text}");
        assert_eq!(text.matches("100%").count(), 13, "{text}");
        for (index, (key, _)) in DOWNLOAD_WORKERS.iter().enumerate() {
            for phase in [
                "starting",
                "installing package",
                "configuring",
                "waiting for engine registration",
                "ready",
            ] {
                state.rows[index].state = if phase == "ready" {
                    RowState::Ready {
                        what: phase.to_string(),
                        elapsed: Duration::from_secs(1),
                    }
                } else {
                    RowState::Starting {
                        what: phase.to_string(),
                        began: Instant::now(),
                    }
                };
                write_terminal(&mut terminal, &state.render(Some((height, width))));
                let text = screen_and_history(&mut terminal);
                assert_eq!(text.matches(key).count(), 1, "{text}");
                assert!(text.contains(&format!("{key} {phase}")), "{text}");
            }
        }
        write_terminal(
            &mut terminal,
            &state.line("compose diagnostic", Some((height, width))),
        );
        state.startup.as_mut().unwrap().finish(true, "Ready");
        write_terminal(&mut terminal, &state.render(Some((height, width))));
        let text = screen_and_history(&mut terminal);
        for (key, _) in DOWNLOAD_WORKERS {
            assert_eq!(text.matches(key).count(), 1, "{text}");
            assert!(text.contains(&format!("✓ {key} ready")), "{text}");
        }
        assert!(!text.contains("100%"), "{text}");
        assert_eq!(text.matches("Engine Ready").count(), 1, "{text}");
        assert_eq!(text.matches("compose diagnostic").count(), 1, "{text}");
    }
}

#[test]
fn static_downloads_report_transitions_not_chunks_or_indentation_changes() {
    for size in [None, Some((3, 80)), Some((24, 20))] {
        let began = Instant::now();
        let mut state = Console {
            downloads: vec![Row {
                key: "provider-anthropic".to_string(),
                depth: 1,
                state: RowState::Downloading {
                    downloaded: 0,
                    total: None,
                    began,
                },
            }],
            ..Console::default()
        };
        let mut output = if size == Some((3, 80)) {
            // A previously animated panel becomes unsafe after a resize.
            state.render(Some((24, 80)));
            state.render(size)
        } else {
            state.render(size)
        };
        for chunk in 1..=512 {
            state.downloads[0].state = RowState::Downloading {
                downloaded: chunk * 16 * 1024,
                total: Some(8 * 1024 * 1024),
                began,
            };
            let update = state.render(size);
            assert!(update.is_empty(), "{size:?}, chunk {chunk}: {update}");
        }
        state.downloads[0].state = RowState::Downloaded {
            downloaded: 8 * 1024 * 1024,
            total: Some(8 * 1024 * 1024),
            elapsed: Duration::from_secs(1),
        };
        output.push_str(&state.render(size));
        state.rows = vec![Row {
            key: "provider-anthropic".to_string(),
            depth: 3,
            state: RowState::Waiting,
        }];
        assert!(
            state.render(size).is_empty(),
            "planning reprinted a download"
        );
        state.rows[0].state = RowState::Starting {
            what: "configuring".to_string(),
            began: Instant::now(),
        };
        output.push_str(&state.render(size));
        state.rows[0].state = RowState::Ready {
            what: "ready".to_string(),
            elapsed: Duration::from_secs(1),
        };
        output.push_str(&state.render(size));
        assert_eq!(output.matches("100%").count(), 1, "{output}");
        assert_eq!(output.matches("configuring").count(), 1, "{output}");
        assert_eq!(
            output.matches("✓ provider-anthropic ready").count(),
            1,
            "{output}"
        );
        assert!(!output.contains('\x1b'), "{output:?}");
    }
}

#[test]
fn completed_download_does_not_claim_worker_readiness() {
    let row = Row {
        key: "api".to_string(),
        depth: 1,
        state: RowState::Downloaded {
            downloaded: 1024,
            total: Some(1024),
            elapsed: Duration::from_secs(1),
        },
    };
    let text = render_row(&row, 0, false);
    assert!(text.contains("100%"), "{text}");
    assert!(!text.contains(OK), "{text}");
}

#[test]
fn startup_preserves_one_ready_line_per_worker_and_engine_at_small_terminal_sizes() {
    for (height, width) in [
        (24, 120),
        (16, 120),
        (15, 120),
        (14, 120),
        (40, 40),
        (24, 40),
    ] {
        let mut state = progress(13);
        let mut terminal = vt100::Parser::new(height, width, 1000);
        for frame in 0..20 {
            state.frame = frame;
            write_terminal(&mut terminal, &state.render(Some((height, width))));
        }
        for index in 0..state.rows.len() {
            state.rows[index].state = RowState::Ready {
                what: "ready".to_string(),
                elapsed: Duration::from_millis(612),
            };
            write_terminal(&mut terminal, &state.render(Some((height, width))));
        }
        state.startup.as_mut().unwrap().finish(true, "Ready");
        write_terminal(&mut terminal, &state.render(Some((height, width))));

        let text = screen_and_history(&mut terminal);
        for name in std::iter::once("Engine Ready".to_string())
            .chain((0..13).map(|index| format!("worker-{index:02} ready")))
        {
            assert_eq!(text.matches(&name).count(), 1, "{width}x{height}: {text}");
        }
    }
}

#[test]
fn adding_workers_beyond_the_screen_keeps_the_confirmed_engine_line_once() {
    let mut state = progress(0);
    let mut terminal = vt100::Parser::new(15, 80, 1000);
    write_terminal(&mut terminal, &state.render(Some((15, 80))));
    state.rows = progress(13).rows;
    for frame in 0..20 {
        state.frame = frame;
        write_terminal(&mut terminal, &state.render(Some((15, 80))));
    }

    let text = screen_and_history(&mut terminal);
    assert_eq!(text.matches("Engine Ready").count(), 1, "{text}");
}

#[test]
fn a_wrapping_status_switches_to_static_output_without_overwriting_prior_lines() {
    let mut state = progress(1);
    let mut terminal = vt100::Parser::new(20, 80, 1000);
    write_terminal(&mut terminal, "keep this log\n");
    write_terminal(&mut terminal, &state.render(Some((20, 80))));
    state.rows[0].state = RowState::Starting {
        what: "waiting for a worker in a very long directory ".repeat(3),
        began: Instant::now(),
    };
    for frame in 0..10 {
        state.frame = frame;
        write_terminal(&mut terminal, &state.render(Some((20, 80))));
    }
    state.rows[0].state = RowState::Ready {
        what: "ready".to_string(),
        elapsed: Duration::from_secs(1),
    };
    write_terminal(&mut terminal, &state.render(Some((20, 80))));

    let text = screen_and_history(&mut terminal);
    assert!(text.starts_with("keep this log\n"), "{text}");
    assert_eq!(text.matches("Engine Ready").count(), 1, "{text}");
    assert_eq!(text.matches("worker-00 ready").count(), 1, "{text}");
}

#[test]
fn smaller_panels_erase_removed_rows() {
    let mut state = progress(3);
    let mut terminal = vt100::Parser::new(24, 80, 1000);
    write_terminal(&mut terminal, &state.render(Some((24, 80))));
    state.rows.truncate(1);
    write_terminal(&mut terminal, &state.render(Some((24, 80))));

    let text = screen_and_history(&mut terminal);
    assert!(text.contains("worker-00"), "{text}");
    assert!(
        !text.contains("worker-01") && !text.contains("worker-02"),
        "{text}"
    );
}

#[test]
fn logs_remain_above_the_panel_even_when_they_wrap_or_span_lines() {
    let mut state = progress(2);
    let mut terminal = vt100::Parser::new(10, 80, 1000);
    write_terminal(&mut terminal, &state.render(Some((10, 80))));
    let message = format!("error: {}\nsecond diagnostic", "long path/".repeat(15));
    write_terminal(&mut terminal, &state.line(&message, Some((10, 80))));
    write_terminal(&mut terminal, &state.render(Some((10, 80))));

    let text = screen_and_history(&mut terminal);
    assert_eq!(text.matches("error:").count(), 1, "{text}");
    assert_eq!(text.matches("second diagnostic").count(), 1, "{text}");
    assert_eq!(text.matches("Engine Ready").count(), 1, "{text}");
}

#[test]
fn logging_during_static_fallback_restores_rows_erased_with_the_previous_panel() {
    let mut state = progress(1);
    let mut terminal = vt100::Parser::new(20, 80, 1000);
    write_terminal(&mut terminal, &state.render(Some((20, 80))));
    state.rows[0].state = RowState::Starting {
        what: "a status that now wraps onto multiple terminal lines ".repeat(3),
        began: Instant::now(),
    };
    write_terminal(&mut terminal, &state.line("new diagnostic", Some((20, 80))));

    let text = screen_and_history(&mut terminal);
    assert_eq!(text.matches("Engine Ready").count(), 1, "{text}");
}

#[test]
fn resizing_stops_cursor_updates_even_when_a_log_arrives_before_the_next_frame() {
    for size in [(3, 80), (24, 25), (40, 120)] {
        let mut state = progress(2);
        let mut terminal = vt100::Parser::new(24, 80, 1000);
        write_terminal(&mut terminal, &state.render(Some((24, 80))));
        terminal.screen_mut().set_size(size.0, size.1);

        let output = state.line("new diagnostic", Some(size));
        assert_eq!(output, "new diagnostic\n");
        write_terminal(&mut terminal, &output);
        assert!(state.render(Some(size)).is_empty());
        // Growing the terminal again must not repaint already committed rows.
        assert!(state.render(Some((40, 120))).is_empty());
    }
}

#[test]
fn missing_dimensions_use_static_state_changes_without_spinner_ticks() {
    let mut state = progress(1);
    let first = state.render(None);
    assert!(first.contains("Engine Ready"), "{first}");
    assert!(!FRAMES.iter().any(|frame| first.contains(frame)), "{first}");
    state.frame += 1;
    assert!(state.render(None).is_empty());
    state.rows[0].state = RowState::Ready {
        what: "ready".to_string(),
        elapsed: Duration::from_millis(25),
    };
    let output = state.render(None);
    assert!(output.contains("worker-00 ready"), "{output}");
    assert!(!output.contains("Engine"), "{output}");
}

#[test]
fn colored_unicode_rows_fit_by_display_width_and_not_ansi_byte_length() {
    let mut state = Console {
        rows: vec![Row {
            key: "\x1b[32m日本語\x1b[0m".to_string(),
            depth: 0,
            state: RowState::Waiting,
        }],
        ..Console::default()
    };
    let mut terminal = vt100::Parser::new(4, 20, 1000);
    write_terminal(&mut terminal, "saved log\n");
    for frame in 0..10 {
        state.frame = frame;
        write_terminal(&mut terminal, &state.render(Some((4, 20))));
    }
    let text = screen_and_history(&mut terminal);
    assert_eq!(text, "saved log\n· 日本語 Pending");
}

#[test]
fn wide_characters_that_would_wrap_do_not_move_the_cursor_into_earlier_logs() {
    let mut state = Console {
        rows: vec![Row {
            key: "a日本語日本語".to_string(),
            depth: 0,
            state: RowState::Waiting,
        }],
        ..Console::default()
    };
    let first = state.render(Some((10, 10)));
    assert!(!first.contains('\x1b'), "{first:?}");
    assert!(state.render(Some((10, 10))).is_empty());
}

fn cancelled_retry_output() -> String {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "report::terminal_tests::cancelled_retry_fixture",
            "--nocapture",
        ])
        .env_remove("CLICOLOR_FORCE")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stderr).unwrap()
}

#[test]
fn cancelled_retries_report_the_final_state() {
    let text = cancelled_retry_output();
    let cancelled = text.find("Containers Cancelled").unwrap();
    assert!(text[cancelled..].contains("api Cancelled"), "{text}");
    assert!(!text[cancelled..].contains("Retrying"), "{text}");
}

#[test]
fn zero_change_summaries_do_not_claim_workers_are_already_running() {
    let text = cancelled_retry_output();
    assert!(text.contains("up: 0 of 2 changed"), "{text}");
    assert!(!text.contains("already in place"), "{text}");
}

#[tokio::test]
#[ignore = "subprocess fixture for cancellation and zero-change summaries"]
async fn cancelled_retry_fixture() {
    let mut progress = StartupProgress::start(true);
    progress.engine_ready();
    progress.downloads_starting();
    containers_starting();
    plan(&[("api".to_string(), 0)]);
    retry_waiting("api", 1, 3, Duration::from_secs(10));
    progress.finish(false, "Cancelled");
    summary_ok("up", 0, 2, Duration::from_millis(100));
}
