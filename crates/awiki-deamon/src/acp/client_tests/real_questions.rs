use super::*;

fn advance(f: &Fixture, previous: &str, next: &str) {
    store::mutate(&f.state, &f.key, None, |session| {
        session.complete(previous, "finished")?;
        let mut task = f.task.clone();
        task.task_id = format!("task_{next}");
        session.submit(Work {
            task,
            run_id: next.into(),
        })?;
        Ok(())
    })
    .unwrap();
}

fn turn(f: &Fixture, run_id: &str, prompt: &str) -> Turn {
    let mut turn = f.turn(prompt);
    turn.run_id = run_id.into();
    turn
}

#[tokio::test]
#[ignore = "requires AWIKI_ACP_REAL_FIXTURE and a configured real model"]
async fn real_client_v2_history_and_questions() {
    let settings: Value = serde_json::from_slice(
        &std::fs::read(std::env::var("AWIKI_ACP_REAL_FIXTURE").unwrap()).unwrap(),
    )
    .unwrap();
    assert!(
        settings["only_case"].is_null() || settings["only_case"] == "run_a",
        "only_case is a first-turn diagnostic; the full flow requires prior turns"
    );
    let mut f = Fixture::new();
    f.profile.driver_id = settings["driver_id"].as_str().unwrap().into();
    f.profile.binary_path = Some(settings["binary_path"].as_str().unwrap().into());
    let probe = inspect(&f.profile).await.unwrap();
    let mut cases = vec![];
    let first = format!("FIRST_{}", rand::random::<u128>());
    let second = format!("SECOND_{}", rand::random::<u128>());
    std::fs::write(f.root.path().join("work/first.txt"), &first).unwrap();
    std::fs::write(f.root.path().join("work/second.txt"), &second).unwrap();
    std::fs::write(f.root.path().join("work/index.txt"), "second.txt").unwrap();
    let mut native = None;
    for (id, prompt, expected) in [
        ("run_a", "Use a file tool to read first.txt in the current directory. Reply with its exact contents only. Do not announce the read or add any commentary before or after using tools.", first.as_str()),
        ("serial", "Use a file tool to read index.txt. It gives a filename in the current directory. Then read that file with a tool and reply with its exact contents only. Do not announce either read or add commentary.", second.as_str()),
        ("memory", "Without tools, repeat exactly the contents of first.txt from our first turn, not the second file.", first.as_str()),
    ] {
        if settings["only_case"].as_str().is_some_and(|only| only != id) { continue; }
        let output = run(turn(&f, id, prompt)).await;
        let snapshot = store::load(&f.state, &f.key).unwrap();
        let exact = output.as_ref().is_ok_and(|o| o.text.trim() == expected);
        let same_native = native.as_ref().is_none_or(|n| Some(n) == snapshot.native_session_id.as_ref());
        native = snapshot.native_session_id;
        cases.push(json!({"case":id,"pass":exact && same_native,"exact_output":exact,"same_native_session":same_native,
            "contains_expected":output.as_ref().is_ok_and(|o|o.text.contains(expected)),
            "output_bytes":output.as_ref().ok().map(|o|o.text.len()),
            "model_id":snapshot.model,"error":output.err().map(|e|e.to_string())}));
        println!("real {id}: {}", exact && same_native);
        advance(&f, id, match id {"run_a"=>"serial","serial"=>"memory",_=>"structured"});
    }
    for mode in ["structured", "custom", "skip"] {
        if settings["only_case"]
            .as_str()
            .is_some_and(|only| only != mode)
        {
            continue;
        }
        let nonce = format!("ANSWER_{}", rand::random::<u128>());
        let prompt = "Use the awiki_questions request_user_input tool once to ask me to choose red or blue, with one required color string enum field. Wait for the real answer. If accepted, reply with exactly the text field from the tool response; if it has no text field reply MISSING_TEXT. If declined, reply exactly SKIPPED and do not ask again. Do not infer or choose an answer.";
        let running = tokio::spawn(run(turn(&f, mode, prompt)));
        let question = tokio::time::timeout(Duration::from_secs(100), async {
            loop {
                let session = store::load(&f.state, &f.key).unwrap();
                if let Some(q) = session.questions.iter().find(|q| q.pending()) {
                    break Some(q.clone());
                }
                if running.is_finished() {
                    break None;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .ok()
        .flatten();
        let started = std::time::Instant::now();
        if mode == "structured" && question.is_some() {
            for _ in 0..65 {
                if running.is_finished() {
                    break;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
        let still_waiting = question.is_some() && !running.is_finished();
        let wait_ms = started.elapsed().as_millis();
        if let Some(question) = question.filter(|_| still_waiting) {
            let mut content = json!({});
            for (key, _) in question.request["requestedSchema"]["properties"]
                .as_object()
                .unwrap()
            {
                content[key] = json!("blue");
            }
            let response = match mode {
                "structured" => {
                    json!({"action":"accept","answer_format":"awiki.answer.v2","mode":mode,"content":content,"text":nonce})
                }
                "custom" => {
                    json!({"action":"accept","answer_format":"awiki.answer.v2","mode":mode,"text":nonce})
                }
                _ => json!({"action":"decline","answer_format":"awiki.answer.v2"}),
            };
            store::mutate(&f.state,&f.key,None,|s|s.command("answer", &json!({"run_id":mode,"question_id":question.id,"definition_hash":question.interaction.unwrap().definition_hash,"response":response}), &f.task.requester_did,current_time_millis()?)).unwrap();
        } else {
            store::mutate(&f.state, &f.key, None, |s| {
                s.stopping = true;
                Ok(())
            })
            .unwrap();
        }
        let output = running.await.unwrap();
        let expected = if mode == "skip" { "SKIPPED" } else { &nonce };
        let exact = output.as_ref().is_ok_and(|o| o.text.trim() == expected);
        let passed = still_waiting && exact && (mode != "structured" || wait_ms >= 65_000);
        cases.push(json!({"case":mode,"pass":passed,"exact_answer":exact,"still_waiting":still_waiting,"wait_ms":wait_ms,"error":output.err().map(|e|e.to_string())}));
        println!("real {mode}: {passed}");
        advance(
            &f,
            mode,
            match mode {
                "structured" => "custom",
                "custom" => "skip",
                _ => "cleanup",
            },
        );
    }
    let report = json!({"driver_id":f.profile.driver_id,"version":probe["binaryVersion"],"platform":std::env::consts::OS,"cases":cases});
    let report_path = PathBuf::from(settings["report_path"].as_str().unwrap())
        .with_extension("questions-v2.json");
    std::fs::write(report_path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    assert!(
        cases.iter().all(|case| case["pass"] == true),
        "real history/question flow failed; see sanitized report"
    );
}
