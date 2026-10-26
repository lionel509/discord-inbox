//! Breaker round 2 (PR #114), break 2: the 21:00 check-in fan-out. The bin sent the check-in with `?` inside a loop over
//! the allowlist, so a failure on the second id left the first id sent, the day marker was removed, and the next 60 s
//! tick sent the first id again. `checkin::send_all` must mark each recipient, the way `summary::send_each` does.
use discord_inbox::checkin;
use std::fs;
use std::path::PathBuf;

const DAY: &str = "2026-10-26";

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("breaker114-r2-checkin-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    root.join("Private").join("Nutrition")
}

/// Break 2: ids [11, 22]; 22 fails on each of three ticks. 11 must be sent exactly once across all three.
/// Then 22 succeeds on the fourth tick: only 22 is sent, and the day's check-in is then done.
#[tokio::test]
async fn a_failing_second_id_does_not_resend_the_checkin_to_the_first() {
    let dir = scratch("dupe");
    let ids = [11u64, 22];
    let mut sent: Vec<u64> = Vec::new();

    for tick in 1..=3 {
        let r = checkin::send_all(&dir, DAY, &ids, |uid, _text| {
            sent.push(uid);
            async move { if uid == 22 { Err("user 22: create_dm failed".to_string()) } else { Ok(()) } }
        })
        .await;
        assert!(r.is_err(), "tick {tick}: user 22 failed, so the call must report the error");
    }
    assert_eq!(
        sent.iter().filter(|&&u| u == 11).count(),
        1,
        "the first id was sent the check-in again on a retry tick: {sent:?}"
    );

    let r4 = checkin::send_all(&dir, DAY, &ids, |uid, _text| {
        sent.push(uid);
        async { Ok(()) }
    })
    .await;
    assert!(r4.is_ok(), "user 22 recovered, so the fourth call must succeed");
    assert_eq!(sent, vec![11, 22, 22, 22, 22], "after the failing ticks only user 22 is sent on the fourth");
    assert_eq!(sent.iter().filter(|&&u| u == 11).count(), 1);
}
