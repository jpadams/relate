//! Seeded synthetic financial-fraud graph.

use rand::rngs::StdRng;
use rand::seq::IndexedRandom;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FraudGraph {
    pub seed: u64,
    pub scale: f64,
    pub customers: Vec<FraudCustomer>,
    pub accounts: Vec<FraudAccount>,
    pub ownership: Vec<(String, String)>,
    pub transfers: Vec<FraudTransfer>,
    pub planted: PlantedPatterns,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FraudCustomer {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FraudAccount {
    pub id: String,
    /// `fanout`, `cycle`, or omitted/empty for ordinary accounts.
    pub planted: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FraudTransfer {
    pub id: String,
    pub from: String,
    pub to: String,
    pub amount: f64,
    pub ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlantedPatterns {
    pub fanout_source: String,
    pub fanout_k: usize,
    pub cycle_accounts: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FraudConfig {
    pub seed: u64,
    pub scale: f64,
    pub fanout_k: usize,
    pub cycle_len: usize,
}

impl Default for FraudConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            scale: 1.0,
            fanout_k: 25,
            cycle_len: 4,
        }
    }
}

impl FraudConfig {
    pub fn ci() -> Self {
        Self {
            scale: 0.05,
            fanout_k: 12,
            cycle_len: 4,
            ..Self::default()
        }
    }

    pub fn from_env() -> Self {
        if std::env::var("RELATE_FULL_DATA").ok().as_deref() == Some("1") {
            Self::default()
        } else {
            Self::ci()
        }
    }
}

pub fn generate(cfg: &FraudConfig) -> FraudGraph {
    let mut rng = StdRng::seed_from_u64(cfg.seed);
    let n_accounts = ((10_000.0 * cfg.scale).round() as usize).max(20);
    let scale_t = cfg.scale.max(0.05);
    let t_min = (10.0 * scale_t).max(2.0);
    let t_max = (1000.0 * scale_t).max(t_min + 1.0);

    let mut customers = Vec::new();
    let mut accounts = Vec::new();
    let mut ownership = Vec::new();
    let mut i = 0usize;
    let mut cust_n = 0usize;
    while i < n_accounts {
        let n_owned = rng.random_range(1..=3.min(n_accounts - i));
        let cid = format!("cust-{cust_n}");
        customers.push(FraudCustomer {
            id: cid.clone(),
            name: format!("Customer {cust_n}"),
        });
        for _ in 0..n_owned {
            let aid = format!("acc-{i}");
            accounts.push(FraudAccount {
                id: aid.clone(),
                planted: None,
            });
            ownership.push((aid, cid.clone()));
            i += 1;
        }
        cust_n += 1;
    }

    let mut transfers = Vec::new();
    let mut tid = 0i64;
    for from_idx in 0..accounts.len() {
        let n = log_uniform(&mut rng, t_min, t_max);
        for _ in 0..n {
            // Never a self-transfer: draw from the other accounts structurally.
            let to_idx = (from_idx + 1 + rng.random_range(0..accounts.len() - 1)) % accounts.len();
            transfers.push(FraudTransfer {
                id: format!("tx-{tid}"),
                from: accounts[from_idx].id.clone(),
                to: accounts[to_idx].id.clone(),
                amount: round_cents(rng.random_range(10.0f64..10_000.0)),
                ts: 1_700_000_000 + tid,
            });
            tid += 1;
        }
    }

    // Planted fan-out: dedicated source with K distinct destinations.
    let fanout_src = "fanout-src".to_string();
    accounts.push(FraudAccount {
        id: fanout_src.clone(),
        planted: Some("fanout".into()),
    });
    let cid = "cust-planted-fanout".to_string();
    customers.push(FraudCustomer {
        id: cid.clone(),
        name: "Planted Fanout".into(),
    });
    ownership.push((fanout_src.clone(), cid));
    // The ordinary pool is exactly the `n_accounts` built above:
    // `accounts[..n_accounts]` are all `planted: None`, and planted accounts
    // are appended after it. Both the destination sampling and the
    // `fanout_k` clamp derive from this one slice.
    let ordinary = &accounts[..n_accounts];
    let fanout_k = cfg.fanout_k.clamp(3, ordinary.len());
    let dests: Vec<String> = ordinary
        .choose_multiple(&mut rng, fanout_k)
        .map(|a| a.id.clone())
        .collect();
    for d in &dests {
        transfers.push(FraudTransfer {
            id: format!("tx-fanout-{tid}"),
            from: fanout_src.clone(),
            to: d.clone(),
            amount: 99.0,
            ts: 1_710_000_000 + tid,
        });
        tid += 1;
    }

    // Planted directed cycle of length 3–6.
    let cycle_len = cfg.cycle_len.clamp(3, 6);
    let mut cycle_accounts = Vec::new();
    let cid = "cust-planted-cycle".to_string();
    customers.push(FraudCustomer {
        id: cid.clone(),
        name: "Planted Cycle".into(),
    });
    for k in 0..cycle_len {
        let id = format!("cycle-{k}");
        accounts.push(FraudAccount {
            id: id.clone(),
            planted: Some("cycle".into()),
        });
        ownership.push((id.clone(), cid.clone()));
        cycle_accounts.push(id);
    }
    for k in 0..cycle_len {
        let from = &cycle_accounts[k];
        let to = &cycle_accounts[(k + 1) % cycle_len];
        transfers.push(FraudTransfer {
            id: format!("tx-cycle-{k}"),
            from: from.clone(),
            to: to.clone(),
            amount: 1000.0 + k as f64,
            ts: 1_720_000_000 + k as i64,
        });
    }

    FraudGraph {
        seed: cfg.seed,
        scale: cfg.scale,
        customers,
        accounts,
        ownership,
        transfers,
        planted: PlantedPatterns {
            fanout_source: fanout_src,
            fanout_k,
            cycle_accounts,
        },
    }
}

/// Round a money amount to whole cents. One implementation for every
/// generator and workload site that quotes a two-decimal amount.
pub(crate) fn round_cents(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn log_uniform(rng: &mut StdRng, min: f64, max: f64) -> usize {
    let lo = min.ln();
    let hi = max.ln();
    let v = (rng.random::<f64>() * (hi - lo) + lo).exp();
    v.round().max(min).min(max) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_cents_keeps_two_decimals() {
        assert_eq!(round_cents(1.234), 1.23);
        assert_eq!(round_cents(1.236), 1.24);
        assert_eq!(round_cents(2_499.999), 2_500.0);
    }

    #[test]
    fn planted_fanout_destinations_are_distinct_ordinary_accounts() {
        let graph = generate(&FraudConfig::ci());
        let dests: Vec<&str> = graph
            .transfers
            .iter()
            .filter(|t| t.from == graph.planted.fanout_source)
            .map(|t| t.to.as_str())
            .collect();
        assert_eq!(dests.len(), graph.planted.fanout_k);
        let mut unique = dests.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), dests.len(), "fan-out destinations repeat");
        for id in dests {
            let account = graph
                .accounts
                .iter()
                .find(|a| a.id == id)
                .expect("fan-out destination is a generated account");
            assert!(account.planted.is_none(), "{id} is not ordinary");
        }
    }
}
