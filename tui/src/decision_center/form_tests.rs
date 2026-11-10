use super::{interaction::Control, *};

fn focus(center: &mut Center, control: Control) {
    for _ in 0..center.controls().len() {
        if center.controls()[center.focus] == control {
            return;
        }
        assert!(center.key_event(KeyCode::Tab).is_none());
    }
    panic!("control is not reachable with Tab");
}
fn intervention() -> Center {
    let mut c = super::tests::center("high");
    let d = c.current.as_mut().unwrap();
    d.request.kind = "intervention".into();
    d.request.question = "Reconnect the device to finish verification".into();
    d.request.options.clear();
    d.request.intervention = Some(serde_json::from_value(json!({"action":"Reconnect the USB device","inaccessible_reason":"No physical access","expected_result":"Report the forwarded port and actual ADB state","verification":"The performer will check independently"})).unwrap());
    d.request.response_fields = vec![dm::ResponseField {
        id: "port_output".into(),
        label: "Forward port and ADB output".into(),
        required: true,
        multiline: true,
        choices: vec![],
    }];
    c.snapshot.as_mut().unwrap()["decisions"] = json!([d.clone()]);
    c
}
#[test]
fn action_result_is_a_visible_form_and_missing_schema_field_gets_focus_before_send() {
    let mut c = intervention();
    let fields = c.form_fields();
    let performed = fields.iter().position(|f| f.id == "performed").unwrap();
    assert!(c.controls()[c.focus] == Control::Field(performed));
    c.key_event(KeyCode::Char(' ')); // explicit confirmation, never preselected
    c.key_event(KeyCode::Tab); // actual result
    for ch in "reconnected !".chars() {
        assert!(c.key_event(KeyCode::Char(ch)).is_none());
    }
    focus(&mut c, Control::Send);
    assert!(c.key_event(KeyCode::Enter).is_none());
    assert!(c.message.contains("Forward port and ADB output"));
    assert!(c.controls()[c.focus] == Control::Field(0));
    assert_eq!(c.drafts["q@1"].fields["result"], "reconnected !");
    for ch in "5037; adb get-state: device".chars() {
        c.key_event(KeyCode::Char(ch));
    }
    focus(&mut c, Control::Send);
    let Some(Action::Answer {
        text,
        fields,
        outcome,
        ..
    }) = c.key_event(KeyCode::Enter)
    else {
        panic!("expected explicit send")
    };
    assert_eq!(fields["performed"], "yes");
    assert_eq!(fields["port_output"], "5037; adb get-state: device");
    assert_eq!(text, "reconnected !");
    assert_eq!(outcome, dm::AnswerOutcome::Answer);
}
#[test]
fn declining_an_intervention_never_claims_it_was_performed() {
    let mut c = intervention();
    focus(&mut c, Control::More);
    c.key_event(KeyCode::Enter);
    c.key_event(KeyCode::Down);
    c.key_event(KeyCode::Down); // cannot do this
    c.key_event(KeyCode::Enter);
    for ch in "I have no USB access either".chars() {
        c.key_event(KeyCode::Char(ch));
    }
    focus(&mut c, Control::Send);
    let Some(Action::Answer {
        fields, outcome, ..
    }) = c.key_event(KeyCode::Enter)
    else {
        panic!("expected honest rejection")
    };
    assert_eq!(outcome, dm::AnswerOutcome::Reject);
    assert!(!fields.contains_key("performed"));
    assert!(!fields.contains_key("result"));
}
#[test]
fn actual_app_routes_modal_keys_and_draws_one_footer_without_function_keys() {
    let (_temp, mut state) = crate::workspace_tests::fixture();
    state.decision_center = intervention();
    for (width, height) in [(40, 12), (80, 24), (110, 36)] {
        let rendered =
            crate::workspace_tests::text(&crate::workspace_tests::render(&state, width, height));
        assert!(rendered.contains("Esc"), "{rendered}");
        assert!(
            rendered.contains("1 yes") && rendered.contains("2 no"),
            "choice values must stay visible on small terminals: {rendered}"
        );
        assert!(!rendered.contains("[F"), "{rendered}");
        assert!(
            !rendered.contains("Commands ·"),
            "duplicate outer command bar: {rendered}"
        );
        crate::workspace_tests::export_buffer("intervention-form", &state, width, height);
    }
    crate::handle_key(&mut state, KeyCode::Tab);
    for ch in "Typed result éà".chars() {
        crate::handle_key(&mut state, KeyCode::Char(ch));
    }
    crate::handle_key(&mut state, KeyCode::Enter);
    assert_eq!(
        state.decision_center.drafts["q@1"].fields["result"],
        "Typed result éà\n"
    );
    assert!(!state.command_palette_open);
    assert!(!state.help_open);
    crate::handle_key(&mut state, KeyCode::Esc);
    assert!(!state.decision_center.open);
}
#[test]
fn advanced_operator_forms_have_a_tab_and_enter_submission_path() {
    let mut c = super::tests::center("high");
    focus(&mut c, Control::More);
    c.key_event(KeyCode::Enter);
    for _ in 0..5 {
        c.key_event(KeyCode::Down);
    }
    c.key_event(KeyCode::Enter); // lifecycle menu
    c.key_event(KeyCode::Down); // withdraw
    c.key_event(KeyCode::Enter);
    for ch in "Obsolete request".chars() {
        c.key_event(KeyCode::Char(ch));
    }
    c.key_event(KeyCode::Tab); // Send
    let Some(Action::Operator {
        intent: dm::OperatorIntent::Withdraw { reason, .. },
        ..
    }) = c.key_event(KeyCode::Enter)
    else {
        panic!("ordinary keys must submit the reviewed action")
    };
    assert_eq!(reason, "Obsolete request");
}

#[test]
fn arrow_editing_respects_unicode_boundaries_and_field_cursors() {
    let mut c = super::tests::center("high");
    for ch in "éà".chars() {
        c.key_event(KeyCode::Char(ch));
    }
    c.key_event(KeyCode::Left);
    c.key_event(KeyCode::Char('X'));
    assert_eq!(c.drafts["q@1"].text, "éXà");
    c.key_event(KeyCode::Backspace);
    c.key_event(KeyCode::Delete);
    assert_eq!(c.drafts["q@1"].text, "é");
    c.key_event(KeyCode::Enter);
    c.key_event(KeyCode::Char('à'));
    c.key_event(KeyCode::Left);
    focus(&mut c, Control::Send);
    focus(&mut c, Control::Note);
    c.key_event(KeyCode::Char('Z'));
    assert_eq!(c.drafts["q@1"].text, "é\nZà");
}

#[test]
fn background_poll_does_not_consume_send_but_a_pending_mutation_blocks_double_submit() {
    let mut c = super::tests::center("high");
    c.key_event(KeyCode::Char('A'));
    let (_tx, rx) = mpsc::channel();
    c.rx = Some(rx);
    focus(&mut c, Control::Send);
    assert!(matches!(
        c.key_event(KeyCode::Enter),
        Some(Action::Answer { .. })
    ));
    c.submitting = true;
    assert!(c.key_event(KeyCode::Enter).is_none());
}

#[test]
fn independent_challenge_is_reachable_without_a_signature_and_cannot_send_an_answer() {
    let mut c = super::tests::center("high");
    c.snapshot.as_mut().unwrap()["policy"]["trust_mode"] = json!("secure_approval");
    for ch in "Exact response for approval".chars() {
        c.key_event(KeyCode::Char(ch));
    }
    focus(&mut c, Control::More);
    c.key_event(KeyCode::Enter);
    for _ in 0..12 {
        c.key_event(KeyCode::Down);
    }
    let Some(Action::Challenge { input, .. }) = c.key_event(KeyCode::Enter) else {
        panic!("prepare the exact challenge without requiring its future signature")
    };
    assert_eq!(input.text, "Exact response for approval");
    assert!(input.attestation.is_none());
}

#[test]
fn task_plan_patch_is_scrollable_and_requires_a_separate_explicit_acceptance() {
    let mut c = super::tests::center("high");
    let operations=(0..30).map(|i|json!({"op":"add_task","task":{"id":format!("NEW-{i}"),"title":format!("Followup {i}")}})).collect::<Vec<_>>();
    c.current.as_mut().unwrap().request.options[0].proposed_patch=Some(serde_json::from_value(json!({"plan_id":"p","base_graph_version":1,"idempotency_key":"review","reason":"Review all effects","operations":operations})).unwrap());
    c.key_event(KeyCode::Right); // choose Keep, never an implicit acceptance
    focus(&mut c, Control::More);
    c.key_event(KeyCode::Enter);
    for _ in 0..10 {
        c.key_event(KeyCode::Down);
    }
    c.key_event(KeyCode::Enter);
    assert!(c.reviewing_patch);
    assert!(!c.drafts["q@1"].apply_patch);
    for _ in 0..5 {
        c.key_event(KeyCode::PageDown);
    }
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| c.draw(f)).unwrap();
    let screen = crate::workspace_tests::text(terminal.backend().buffer());
    assert!(
        screen.contains("NEW-29"),
        "the full patch must be scrollable: {screen}"
    );
    c.key_event(KeyCode::Enter); // Back is selected initially
    assert!(!c.reviewing_patch);
    assert!(!c.drafts["q@1"].apply_patch);
    focus(&mut c, Control::More);
    c.key_event(KeyCode::Enter);
    for _ in 0..10 {
        c.key_event(KeyCode::Down);
    }
    c.key_event(KeyCode::Enter);
    c.key_event(KeyCode::Tab);
    c.key_event(KeyCode::Enter);
    assert!(c.drafts["q@1"].apply_patch);
    focus(&mut c, Control::Send);
    assert!(matches!(
        c.key_event(KeyCode::Enter),
        Some(Action::Answer {
            apply_patch: true,
            ..
        })
    ));
}

#[test]
fn numbered_choice_goes_straight_to_send_without_an_extra_note() {
    let mut c = super::tests::center("high");
    c.key_event(KeyCode::Char('1'));
    assert!(c.controls()[c.focus] == Control::Send);
    let Some(Action::Answer { text, option, .. }) = c.key_event(KeyCode::Enter) else {
        panic!("choose then Send")
    };
    assert_eq!(text, "Keep");
    assert_eq!(option.as_deref(), Some("keep"));
}
#[test]
fn a_single_typed_yes_no_field_is_the_answer_not_a_second_free_text_question() {
    let mut c = super::tests::center("high");
    c.current.as_mut().unwrap().request.options.clear();
    c.current.as_mut().unwrap().request.response_fields = vec![dm::ResponseField {
        id: "ready".into(),
        label: "Is access restored?".into(),
        required: true,
        multiline: false,
        choices: vec!["yes".into(), "no".into()],
    }];
    c.key_event(KeyCode::Char('1'));
    assert!(c.controls()[c.focus] == Control::Send);
    let Some(Action::Answer { text, fields, .. }) = c.key_event(KeyCode::Enter) else {
        panic!("yes is enough for a declared binary question")
    };
    assert_eq!(text, "yes");
    assert_eq!(fields["ready"], "yes");
}
#[test]
fn recorded_answer_closes_the_popup_but_failure_and_pending_approval_do_not() {
    for outcome in ["resolved", "pending_human", "error", "approval_waiting"] {
        let mut c = super::tests::center("high");
        let mut fresh = c.current.clone().unwrap();
        if matches!(outcome, "resolved" | "approval_waiting") {
            fresh.status = "resolved".into();
            fresh.version = 2;
            fresh.resolution=Some(serde_json::from_value(json!({"answered_version":1,"text":"yes","selected_option_id":null,"actor":"operator","origin":"human_local_trust","policy_digest":"p","recorded_at":"2026-11-07T10:00:00Z"})).unwrap());
        }
        let (tx, rx) = mpsc::channel();
        c.rx = Some(rx);
        c.last = Some(Instant::now());
        c.submitting = true;
        c.answer_in_flight = Some(("q".into(), 1));
        tx.send(if outcome == "error" {
            Err("validation failed".into())
        } else {
            Ok(json!({"policy":c.policy(),"decisions":[fresh],"selected_decision_id":if outcome=="resolved" {Some("q")} else {None}}))
        })
        .unwrap();
        c.tick(None, false);
        assert_eq!(c.open, outcome != "resolved", "{outcome}");
        if outcome == "resolved" {
            c.tick(None, false);
            assert!(!c.open);
        }
    }
}
#[test]
fn recorded_history_does_not_show_pending_form_instructions_or_duplicate_action() {
    let mut c = intervention();
    let d = c.current.as_mut().unwrap();
    d.status = "resolved".into();
    d.resolution=Some(serde_json::from_value(json!({"text":"yes","selected_option_id":null,"actor":"operator","origin":"human_local_trust","policy_digest":"p","recorded_at":"2026-11-07T10:00:00Z"})).unwrap());
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| c.draw(f)).unwrap();
    let screen = crate::workspace_tests::text(terminal.backend().buffer());
    assert!(!screen.contains("Action needed"));
    assert!(!screen.contains("Required details"));
    assert!(!screen.contains("Choose an option"));
    assert!(!screen.contains("Confirm the action"));
    assert!(screen.contains("Close"));
}

#[test]
fn intervention_choices_precede_evidence_and_typed_deferral_needs_no_fake_result() {
    let mut c = intervention();
    c.current.as_mut().unwrap().request.options=serde_json::from_value(json!([
        {"id":"provide","label":"Provide device access","intervention_response":"completed"},
        {"id":"defer","label":"Keep observations unverified","intervention_response":"defer_observation"}
    ])).unwrap();
    c.snapshot.as_mut().unwrap()["decisions"] = json!([c.current.clone().unwrap()]);
    c.focus = 0;
    assert!(c.controls()[0] == Control::Option);
    c.key_event(KeyCode::Char('2'));
    assert!(c.controls()[c.focus] == Control::Send);
    assert_eq!(
        c.response_mode(),
        dm::InterventionResponse::DeferObservation
    );
    assert!(!c.controls().iter().any(|c| matches!(c, Control::Field(_))));
    focus(&mut c, Control::Send);
    let Some(Action::Answer {
        option,
        fields,
        text,
        ..
    }) = c.key_event(KeyCode::Enter)
    else {
        panic!("deferral must send without an invented action");
    };
    assert_eq!(option.as_deref(), Some("defer"));
    assert!(!text.is_empty());
    assert!(!fields.contains_key("performed"));
    assert!(!fields.contains_key("result"));
}
#[test]
fn explicit_mode_does_not_silently_relabel_a_legacy_provide_choice() {
    let mut c = intervention();
    c.current.as_mut().unwrap().request.options =
        serde_json::from_value(json!([{"id":"provide","label":"Provide isolated access"}]))
            .unwrap();
    c.focus = 0;
    c.key_event(KeyCode::Char('1'));
    assert!(c.controls()[c.focus] == Control::Mode);
    c.key_event(KeyCode::Char('3'));
    assert_eq!(c.drafts["q@1"].option, 0);
    assert_eq!(
        c.drafts["q@1"].fields[dm::RESPONSE_MODE],
        "defer_observation"
    );
    assert!(!c.controls().iter().any(|c| matches!(c, Control::Field(_))));
}
#[test]
fn multiline_help_matches_the_actual_enter_key_and_delivery_requires_exact_version() {
    let mut c = intervention();
    assert!(c.keyboard_hint(Control::Field(0)).contains("New line"));
    let d = c.current.as_mut().unwrap();
    d.status = "resolved".into();
    d.version = 2;
    d.resolution=Some(serde_json::from_value(json!({"text":"Device connected","actor":"Human","origin":"operator","policy_digest":"p","recorded_at":"now","selected_option_id":null,"advisory":false,"answered_version":1})).unwrap());
    c.snapshot.as_mut().unwrap()["delivery"] = json!([{"task":"A","phase":"review","current":false,"delivered":true,"acknowledged":true,"decisions":[{"id":"q","version":1}],"compliance":{"q":{"status":"pass"}}}]);
    assert!(c.delivery_overview().contains("Delivery pending"));
    c.snapshot.as_mut().unwrap()["delivery"][0]["decisions"][0]["version"] = json!(2);
    assert!(c.delivery_overview().contains("context must be refreshed"));
}

#[test]
fn polling_a_different_answer_version_cannot_close_the_submitted_form() {
    for answered in [None, Some(2)] {
        let mut c = super::tests::center("high");
        let mut fresh = c.current.clone().unwrap();
        fresh.status = "resolved".into();
        fresh.version = 3;
        fresh.resolution=Some(serde_json::from_value(json!({"answered_version":answered,"text":"yes","selected_option_id":null,"actor":"operator","origin":"human_local_trust","policy_digest":"p","recorded_at":"2026-11-08T10:00:00Z"})).unwrap());
        let (tx, rx) = mpsc::channel();
        c.rx = Some(rx);
        c.last = Some(Instant::now());
        c.submitting = true;
        c.answer_in_flight = Some(("q".into(), 1));
        tx.send(Ok(
            json!({"policy":c.policy(),"decisions":[fresh],"selected_decision_id":"q"}),
        ))
        .unwrap();
        c.tick(None, false);
        assert!(c.open, "must correlate answered_version, not just status");
    }
}

#[test]
fn blocked_external_is_visible_and_cannot_look_like_a_recorded_answer() {
    let mut center = intervention();
    center.current.as_mut().unwrap().status = "blocked_external".into();
    center.snapshot.as_mut().unwrap()["decisions"] = json!([center.current.clone().unwrap()]);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| center.draw(frame)).unwrap();
    let text = crate::workspace_tests::text(terminal.backend().buffer());
    assert!(text.contains("Blocked: prerequisite unavailable"));
    assert!(text.contains("BLOCKED: external intervention"));
    assert!(!text.contains("Recorded decision"));
    assert!(!text.contains("Recorded response"));
}
