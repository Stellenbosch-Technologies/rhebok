use super::choice::QWEN3;
use super::Embedder;
use anyhow::{ensure, Result};

pub const PAIRS: [(&str, &str); 24] = [
    (
        "how do we roll back a mobile release that crashes on launch?",
        "# Mobile releases\n\nIf a release crashes on launch, halt the staged rollout in the store console. Then ship the previous build again with a higher version number.",
    ),
    (
        "how long should the brisket rest after smoking?",
        "# Smoking meat\n\nWrap the brisket in butcher paper and rest it in a cooler for at least two hours after it comes off the smoker, or the juices run out when you slice it.",
    ),
    (
        "why does autovacuum fall behind on the events table?",
        "# Postgres tuning\n\nThe events table bloats because the default autovacuum scale factor is far too high for a table that size. Setting it to 0.01 for that table fixed the lag.",
    ),
    (
        "how do I strip debug symbols from the release binary?",
        "# Rust release profile\n\nSet strip = true under [profile.release] in Cargo.toml. The release binary drops its debug symbols and shrinks by about a third.",
    ),
    (
        "when is the quarterly board meeting?",
        "# Board calendar\n\nThe quarterly board meeting is on the second Tuesday after quarter end. The finance lead sends the deck a week before.",
    ),
    (
        "how do we rotate the load balancer certificate?",
        "# TLS runbook\n\nUpload the new certificate to the secret store, point the listener at it and reload. Connections are not dropped during the reload.",
    ),
    (
        "which painkiller must the dog never get?",
        "# Vet visit\n\nThe vet said the dog must never get ibuprofen, at any dose. For pain, give only the carprofen she prescribed, with food.",
    ),
    (
        "how often are failed webhooks retried?",
        "# Webhook delivery\n\nA failed delivery is retried with exponential backoff, five attempts over about an hour. After that the event goes to the dead-letter queue.",
    ),
    (
        "how much sleep helps memory, according to that study?",
        "# Reading notes: sleep\n\nThe study linked seven to nine hours of sleep a night to better memory consolidation than six hours or less.",
    ),
    (
        "what torque for the bike stem bolts?",
        "# Bike maintenance\n\nTighten the stem bolts to 5 Nm in a cross pattern with a torque wrench. Too much torque can crack a carbon steerer.",
    ),
    (
        "who signed off on moving CI to GitHub Actions?",
        "# Decision log\n\nThe move from Jenkins to GitHub Actions was approved by the platform lead in March, after the cost review.",
    ),
    (
        "what is the largest file the upload API accepts?",
        "# API limits\n\nThe upload endpoint rejects files over 25 MB with a 413. Uploads above 10 MB must use the multipart flow.",
    ),
    (
        "when does my passport expire?",
        "# Travel documents\n\nMy passport expires in August 2028. Some countries need six months of validity left, so renew it before February 2028.",
    ),
    (
        "what did we decide about the office lease?",
        "# Office lease\n\nWe decided not to renew the lease on the second floor. The team moves to the smaller space on the ground floor in June.",
    ),
    (
        "how do I bleed the radiators?",
        "# Home heating\n\nTurn the heating off, open the bleed valve at the top of each radiator with the key until water comes out, then top the boiler pressure back up to 1.5 bar.",
    ),
    (
        "what's the on-call escalation if the primary doesn't answer?",
        "# On-call policy\n\nIf the primary does not acknowledge a page in fifteen minutes, it escalates to the secondary, and after thirty minutes to the engineering manager.",
    ),
    (
        "how much did the kitchen renovation cost in the end?",
        "# Kitchen renovation\n\nThe final cost was 18,400, about 2,000 over the quote, mostly because the old wiring had to be replaced.",
    ),
    (
        "which index makes the user search query fast?",
        "# Search performance\n\nA trigram GIN index on users.email took the search query from two seconds to under twenty milliseconds.",
    ),
    (
        "when should I plant tomato seedlings outside?",
        "# Garden\n\nPlant the tomato seedlings out after the last frost, usually mid-May here, once nights stay above ten degrees.",
    ),
    (
        "what's our refund window for annual plans?",
        "# Billing policy\n\nAnnual plans can be refunded in full within thirty days of purchase. After that we refund the unused months on request.",
    ),
    (
        "how do I reset the router to factory settings?",
        "# Home network\n\nHold the reset button on the back of the router for ten seconds until the lights flash. The admin password goes back to the one on the sticker.",
    ),
    (
        "what feedback did the reviewer give on the API design doc?",
        "# Design review notes\n\nThe reviewer asked us to version the API in the URL, not a header, and to return 404 rather than 403 for resources the caller cannot see.",
    ),
    (
        "how many grams of protein should I eat a day?",
        "# Nutrition notes\n\nThe dietitian suggested about 1.6 grams of protein per kilogram of body weight a day while training, spread over four meals.",
    ),
    (
        "why did the nightly backup job fail last week?",
        "# Incident: nightly backup\n\nThe nightly backup failed because the disk on the backup host filled up. Old snapshots were not being pruned after the retention change.",
    ),
];

pub const HOOK_SPREADS: f64 = 1.29;
pub const MCP_SPREADS: f64 = -1.19;

#[derive(Debug, Clone, PartialEq)]
pub struct GateEstimate {
    pub hook: f64,
    pub mcp: f64,
    pub measured_hook: f64,
    pub measured_mcp: f64,
    pub unrelated_mean: f64,
    pub pairs: usize,
    pub matched_below_hook: usize,
}

impl GateEstimate {
    pub fn separates(&self) -> bool {
        self.matched_below_hook * 4 <= self.pairs
    }

    pub fn hook_kept_qwen3s(&self) -> bool {
        self.measured_hook < self.hook
    }

    pub fn mcp_kept_qwen3s(&self) -> bool {
        self.measured_mcp < self.mcp
    }
}

pub fn relevance(a: &[f32], b: &[f32]) -> f64 {
    let dot: f64 = a.iter().zip(b).map(|(x, y)| *x as f64 * *y as f64).sum();
    ((1.0 + dot) / 2.0).clamp(0.0, 1.0)
}

fn two_places(x: f64) -> f64 {
    (x.clamp(0.0, 1.0) * 100.0).round() / 100.0
}

pub fn estimate(matched: &[f64], unrelated: &[f64]) -> Option<GateEstimate> {
    if matched.is_empty() || unrelated.len() < 2 {
        return None;
    }
    let n = unrelated.len() as f64;
    let mean = unrelated.iter().sum::<f64>() / n;
    let sd = (unrelated.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n).sqrt();
    let measured_hook = two_places(mean + HOOK_SPREADS * sd);
    let measured_mcp = two_places(mean + MCP_SPREADS * sd).min(measured_hook);
    let hook = measured_hook.max(QWEN3.hook_threshold);
    let mcp = measured_mcp.max(QWEN3.mcp_threshold);
    Some(GateEstimate {
        hook,
        mcp,
        measured_hook,
        measured_mcp,
        unrelated_mean: two_places(mean),
        pairs: matched.len(),
        matched_below_hook: matched.iter().filter(|m| **m < hook).count(),
    })
}

pub fn scores(queries: &[Vec<f32>], notes: &[Vec<f32>]) -> (Vec<f64>, Vec<f64>) {
    let mut matched = Vec::with_capacity(queries.len());
    let mut unrelated = Vec::with_capacity(queries.len() * notes.len());
    for (i, q) in queries.iter().enumerate() {
        for (j, n) in notes.iter().enumerate() {
            if i == j {
                matched.push(relevance(q, n));
            } else {
                unrelated.push(relevance(q, n));
            }
        }
    }
    (matched, unrelated)
}

pub fn measure(embedder: &dyn Embedder) -> Result<GateEstimate> {
    let notes = embedder.embed_documents(
        &PAIRS
            .iter()
            .map(|(_, note)| note.to_string())
            .collect::<Vec<_>>(),
    )?;
    let queries = PAIRS
        .iter()
        .map(|(query, _)| embedder.embed_query(query))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        notes.len() == PAIRS.len(),
        "the model returned {} embeddings for {} notes",
        notes.len(),
        PAIRS.len()
    );
    let (matched, unrelated) = scores(&queries, &notes);
    estimate(&matched, &unrelated).ok_or_else(|| anyhow::anyhow!("the probe produced no scores"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gates_sit_where_qwen3s_sit_in_its_own_unrelated_scores() {
        let unrelated: Vec<f64> = (0..552)
            .map(|i| 0.6028 + 0.0443 * if i % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        let e = estimate(&[0.88; 24], &unrelated).unwrap();
        assert_eq!(e.hook, QWEN3.hook_threshold);
        assert_eq!(e.mcp, QWEN3.mcp_threshold);
    }

    #[test]
    fn a_model_that_scores_unrelated_text_high_gets_a_higher_gate() {
        let unrelated: Vec<f64> = (0..552)
            .map(|i| 0.734 + 0.028 * if i % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        let e = estimate(&[0.89; 24], &unrelated).unwrap();
        assert_eq!(e.hook, 0.77);
        assert_eq!(e.mcp, 0.70);
        assert_eq!(e.unrelated_mean, 0.73);
        assert!(!e.hook_kept_qwen3s() && !e.mcp_kept_qwen3s());
        assert!(e.separates());
    }

    #[test]
    fn a_model_that_scores_unrelated_text_low_never_gets_a_looser_gate_than_qwen3s() {
        let unrelated: Vec<f64> = (0..552)
            .map(|i| 0.523 + 0.043 * if i % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        let e = estimate(&[0.83; 24], &unrelated).unwrap();
        assert_eq!((e.hook, e.mcp), (0.66, 0.55));
        assert_eq!((e.measured_hook, e.measured_mcp), (0.58, 0.47));
        assert!(e.hook_kept_qwen3s() && e.mcp_kept_qwen3s());
    }

    #[test]
    fn each_gate_is_floored_on_its_own() {
        let unrelated: Vec<f64> = (0..552)
            .map(|i| 0.62 + 0.07 * if i % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        let e = estimate(&[0.9; 24], &unrelated).unwrap();
        assert_eq!((e.hook, e.mcp), (0.71, 0.55));
        assert!(!e.hook_kept_qwen3s());
        assert!(e.mcp_kept_qwen3s());
    }

    #[test]
    fn matched_pairs_under_the_hook_gate_mean_the_model_does_not_separate() {
        let unrelated = [0.70, 0.80];
        let e = estimate(&[0.70; 24], &unrelated).unwrap();
        assert_eq!(e.matched_below_hook, 24);
        assert!(!e.separates());
    }

    #[test]
    fn no_scores_make_no_estimate() {
        assert_eq!(estimate(&[], &[0.5, 0.6]), None);
        assert_eq!(estimate(&[0.9], &[0.5]), None);
    }

    #[test]
    fn each_query_is_matched_with_its_own_note_and_unrelated_to_the_rest() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let (matched, unrelated) = scores(&[a.clone(), b.clone()], &[a, b]);
        assert_eq!(matched, [1.0, 1.0]);
        assert_eq!(unrelated, [0.5, 0.5]);
    }

    #[test]
    fn the_probe_queries_are_distinct_and_every_note_has_a_heading() {
        let mut queries: Vec<&str> = PAIRS.iter().map(|(q, _)| *q).collect();
        queries.sort();
        queries.dedup();
        assert_eq!(queries.len(), PAIRS.len());
        assert!(PAIRS.iter().all(|(_, n)| n.starts_with("# ")));
    }
}
