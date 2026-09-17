use super::*;
use crate::acp::questions::{validate_response, QuestionInteraction};

fn question(shared: bool) -> Question {
    let request = json!({"mode":"form","message":"Choose","requestedSchema":{"type":"object","required":["choice"],"properties":{"choice":{"type":"string","enum":["a","b"]}}}});
    Question {
        id: "q".into(),
        run_id: "run_a".into(),
        expires_at_ms: i64::MAX,
        interaction: Some(QuestionInteraction::new(&request, shared).unwrap()),
        request,
        response: None,
        end_reason: None,
    }
}
fn args(question: &Question, response: Value) -> Value {
    json!({"action":"answer","run_id":question.run_id,"question_id":question.id,
        "definition_hash":question.interaction.as_ref().unwrap().definition_hash,"response":response})
}

#[test]
fn shared_answers_preserve_choices_supplement_and_custom_unicode_exactly() {
    let q = question(true);
    for response in [
        json!({"action":"accept","answer_format":"awiki.answer.v2","mode":"structured","content":{"choice":"b"},"text":"  补充\n不要省略。  "}),
        json!({"action":"accept","answer_format":"awiki.answer.v2","mode":"custom","text":"  自定义\n与选项不同。  "}),
        json!({"action":"decline","answer_format":"awiki.answer.v2"}),
    ] {
        assert_eq!(
            validate_response(&q, &args(&q, response.clone())).unwrap(),
            response
        );
    }
    let legacy = validate_response(
        &q,
        &json!({"response":{"action":"accept","content":{"choice":"a"}}}),
    )
    .unwrap();
    assert_eq!(legacy["mode"], "structured");
    assert_eq!(legacy["content"]["choice"], "a");
}

#[test]
fn invalid_or_unadvertised_answer_content_is_never_silently_dropped() {
    let q = question(true);
    for response in [
        json!({"action":"accept","answer_format":"unknown","content":{"choice":"a"}}),
        json!({"action":"accept","text":"lost","content":{"choice":"a"}}),
        json!({"action":"decline","content":{"choice":"a"}}),
        json!({"action":"cancel","answer_format":"awiki.answer.v2"}),
        json!({"action":"accept","answer_format":"awiki.answer.v2","mode":"custom","content":{},"text":"hidden choices"}),
        json!({"action":"accept","answer_format":"awiki.answer.v2","mode":"custom","text":" \n "}),
        json!({"action":"accept","answer_format":"awiki.answer.v2","mode":"custom","text":"字".repeat(5462)}),
        json!({"action":"accept","answer_format":"awiki.answer.v2","mode":"structured","content":{"choice":"invalid"},"text":"cannot bypass schema"}),
    ] {
        assert!(validate_response(&q, &args(&q, response)).is_err());
    }
    let custom = json!({"action":"accept","answer_format":"awiki.answer.v2","mode":"custom","text":"answer"});
    let mut command = args(&q, custom.clone());
    command["definition_hash"] = "changed".into();
    assert!(validate_response(&q, &command).is_err());
    command.as_object_mut().unwrap().remove("definition_hash");
    assert!(validate_response(&q, &command).is_err());
    let native = question(false);
    assert!(validate_response(&native, &args(&native, custom)).is_err());
    assert!(validate_response(&native,&args(&native,json!({"action":"accept","answer_format":"awiki.answer.v2","mode":"structured","content":{"choice":"a"},"text":"not supported"}))).is_err());
    assert_eq!(
        validate_response(&native, &json!({"response":{"action":"cancel"}})).unwrap(),
        json!({"action":"cancel"})
    );
}

#[test]
fn answer_and_stop_keep_the_winning_fact_and_durable_closed_reason() {
    for first_answer in [true, false] {
        let (_root, state, seed) = fixture();
        let q = question(true);
        mutate(&state, &seed.key, None, |s| {
            s.questions.push(q.clone());
            Ok(())
        })
        .unwrap();
        let mut command = args(
            &q,
            json!({"action":"accept","answer_format":"awiki.answer.v2","mode":"custom","text":"real answer"}),
        );
        let initial = load(&state, &seed.key).unwrap();
        command["session_key"] = json!(seed.key);
        command["revision"] = json!(initial.revision);
        if first_answer {
            control(&state, &seed.agent_did, "did:human:alice", "a", &command).unwrap();
        }
        mutate(&state, &seed.key, None, |s| {
            s.command("stop", &json!({"run_id":"run_a"}), "did:human:alice", 1)
        })
        .unwrap();
        if !first_answer {
            assert!(control(&state, &seed.agent_did, "did:human:alice", "a", &command).is_err());
        }
        mutate(&state, &seed.key, None, |s| {
            s.complete("run_a", "cancelled")
        })
        .unwrap();
        let record = crate::acp::task_records::load(&state.connection().unwrap(), "run_a")
            .unwrap()
            .unwrap();
        assert_eq!(
            record.questions[0]["status"],
            if first_answer { "answered" } else { "closed" }
        );
        if first_answer {
            assert_eq!(record.questions[0]["response"]["text"], "real answer");
            assert!(control(&state, &seed.agent_did, "did:human:alice", "a", &command).is_ok());
        } else {
            assert_eq!(record.questions[0]["end_reason"], "task_stopped");
        }
    }
}

#[test]
fn expiry_poll_rechecks_the_committed_answer_and_close_reason() {
    use crate::acp::questions::expire_or_answer;
    let mut q = question(true);
    q.expires_at_ms = 100;
    let answer = json!({"action":"accept","answer_format":"awiki.answer.v2","mode":"custom","text":"committed just before deadline"});
    q.response = Some(answer.clone());
    assert_eq!(expire_or_answer(&mut q, 101).unwrap(), Some(answer));
    assert!(q.end_reason.is_none());
    q.response = None;
    q.end_reason = Some("task_stopped".into());
    assert!(expire_or_answer(&mut q, 101).is_err());
    assert_eq!(q.end_reason.as_deref(), Some("task_stopped"));
    q.end_reason = None;
    assert!(expire_or_answer(&mut q, 99).unwrap().is_none());
    assert!(q.pending());
    assert!(expire_or_answer(&mut q, 100).unwrap().is_none());
    assert_eq!(q.end_reason.as_deref(), Some("expired"));
}
