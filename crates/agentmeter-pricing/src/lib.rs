//! Reversible AgentMeter pricing of immutable token facts.
//!
//! Estimates are computed from versioned, reviewed rate datasets using exact
//! integer nano-USD arithmetic. Token facts are never modified; estimates are
//! disposable facts that can be replaced wholesale when a new dataset
//! arrives. Unknown or ambiguous models stay unpriced — this crate never
//! guesses the nearest expensive model.

use std::collections::BTreeMap;

use agentmeter_core::{CostKind, DataConfidence, NanoUsd, TokenBreakdown};

pub const BUNDLED_SOURCE: &str = "agentmeter-reviewed";
pub const BUNDLED_VERSION: &str = "2026-08-19.1";

/// Verified seed rates, nano-USD per token (USD-per-million × 1_000). Every
/// entry and its documented inferences are recorded with sources in
/// `docs/research/rates-2026-08-19.md`.
const SEED_RATES: &[(&str, ModelRates)] = &[
    // developers.openai.com/api/docs/pricing — gpt-5.3-codex, Standard tier.
    // Cache-write bills as regular input (no write premium listed); one
    // output price, so reasoning is priced as output. Fast-mode usage is
    // underestimated until tiers are distinguishable.
    (
        "gpt-5.3-codex",
        ModelRates {
            input: 1_750,
            output: 14_000,
            cache_read: 175,
            cache_write: 1_750,
            reasoning: 14_000,
        },
    ),
    // developers.openai.com/api/docs/pricing — gpt-5.6 family with explicit
    // cache-write columns; reasoning priced as output.
    (
        "gpt-5.6-terra",
        ModelRates {
            input: 2_000,
            output: 12_000,
            cache_read: 200,
            cache_write: 2_500,
            reasoning: 12_000,
        },
    ),
    (
        "gpt-5.6-sol",
        ModelRates {
            input: 5_000,
            output: 30_000,
            cache_read: 500,
            cache_write: 6_250,
            reasoning: 30_000,
        },
    ),
    (
        "gpt-5.6-luna",
        ModelRates {
            input: 200,
            output: 1_200,
            cache_read: 20,
            cache_write: 250,
            reasoning: 1_200,
        },
    ),
    (
        "gpt-5.6-cyber",
        ModelRates {
            input: 12_500,
            output: 75_000,
            cache_read: 1_250,
            cache_write: 15_625,
            reasoning: 75_000,
        },
    ),
    // platform.kimi.ai/docs/pricing — cache miss/hit listed; cache write
    // bills as a cache miss, reasoning inside output.
    (
        "kimi-k2.7-code",
        ModelRates {
            input: 950,
            output: 4_000,
            cache_read: 190,
            cache_write: 950,
            reasoning: 4_000,
        },
    ),
    (
        "kimi-k2.7-code-highspeed",
        ModelRates {
            input: 1_900,
            output: 8_000,
            cache_read: 380,
            cache_write: 1_900,
            reasoning: 8_000,
        },
    ),
    (
        "kimi-k3",
        ModelRates {
            input: 3_000,
            output: 15_000,
            cache_read: 300,
            cache_write: 3_000,
            reasoning: 15_000,
        },
    ),
    // platform.claude.com/docs/en/about-claude/pricing — 5-minute cache
    // write tier; thinking billed at the output rate per official docs.
    // claude-sonnet-4-7 carries the time-limited discount ending
    // 2026-09-30.
    (
        "claude-opus-5-5",
        ModelRates {
            input: 45_000,
            output: 225_000,
            cache_read: 4_500,
            cache_write: 56_250,
            reasoning: 225_000,
        },
    ),
    (
        "claude-sonnet-4-7",
        ModelRates {
            input: 3_000,
            output: 15_000,
            cache_read: 300,
            cache_write: 3_750,
            reasoning: 15_000,
        },
    ),
    (
        "claude-haiku-4-6",
        ModelRates {
            input: 1_000,
            output: 5_000,
            cache_read: 100,
            cache_write: 1_250,
            reasoning: 5_000,
        },
    ),
    (
        "claude-opus-4-5",
        ModelRates {
            input: 15_000,
            output: 75_000,
            cache_read: 1_500,
            cache_write: 18_750,
            reasoning: 75_000,
        },
    ),
    (
        "claude-opus-4-1",
        ModelRates {
            input: 15_000,
            output: 75_000,
            cache_read: 1_500,
            cache_write: 18_750,
            reasoning: 75_000,
        },
    ),
    (
        "claude-sonnet-4-5",
        ModelRates {
            input: 3_000,
            output: 15_000,
            cache_read: 300,
            cache_write: 3_750,
            reasoning: 15_000,
        },
    ),
    (
        "claude-haiku-4-5",
        ModelRates {
            input: 1_000,
            output: 5_000,
            cache_read: 100,
            cache_write: 1_250,
            reasoning: 5_000,
        },
    ),
    // api-docs.deepseek.com/quick_start/pricing — peak-tier prices; the
    // half-price off-peak windows are not yet modeled, so off-peak usage is
    // overestimated.
    (
        "deepseek-v4-flash",
        ModelRates {
            input: 440,
            output: 1_320,
            cache_read: 14,
            cache_write: 440,
            reasoning: 1_320,
        },
    ),
    (
        "deepseek-v4-pro",
        ModelRates {
            input: 1_320,
            output: 3_960,
            cache_read: 44,
            cache_write: 1_320,
            reasoning: 3_960,
        },
    ),
];

/// Reviewed aliases; both mappings are official. Kimi documents
/// `kimi-for-coding` as the CLI alias of `kimi-k2.7-code`
/// (kimi.com/code/docs/en), and DeepSeek's changelog maps the retired
/// `deepseek-chat`/`deepseek-reasoner` ids onto `deepseek-v4-flash` modes.
const SEED_ALIASES: &[(&str, &str)] = &[
    ("kimi-for-coding", "kimi-k2.7-code"),
    ("kimi-for-coding-highspeed", "kimi-k2.7-code-highspeed"),
    ("deepseek-chat", "deepseek-v4-flash"),
    ("deepseek-reasoner", "deepseek-v4-flash"),
];

/// Integer nano-USD per token. Rates like $3 per million input tokens are
/// exactly 3_000 nano-USD per token, so per-token rates stay integral for
/// realistic catalog prices and estimation never touches floating point.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ModelRates {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub reasoning: u64,
}

impl ModelRates {
    /// Exact per-bucket cost, or `None` when the multiplication would
    /// overflow a `u64`; overflow is surfaced instead of silently truncated.
    pub fn cost(self, tokens: TokenBreakdown) -> Option<NanoUsd> {
        let buckets = [
            (tokens.input, self.input),
            (tokens.output, self.output),
            (tokens.cache_read, self.cache_read),
            (tokens.cache_write, self.cache_write),
            (tokens.reasoning, self.reasoning),
        ];
        let mut nanos = 0_u64;
        for (tokens, rate) in buckets {
            nanos = nanos.checked_add(tokens.checked_mul(rate)?)?;
        }
        Some(NanoUsd::from_nanos(nanos))
    }
}

/// A reviewed, versioned rate dataset. `rates` keys are canonical model ids,
/// optionally provider-qualified as `provider/model`; `aliases` map known
/// alternative spellings to canonical keys.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RateDataset {
    pub source: String,
    pub version: String,
    pub rates: BTreeMap<String, ModelRates>,
    pub aliases: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RateMatch<'a> {
    Exact { key: &'a str },
    Alias { alias: &'a str, canonical: &'a str },
}

impl RateDataset {
    /// The dataset shipped with the application, seeded only from rates
    /// verified against official provider pricing pages on 2026-08-19 (see
    /// `docs/research/rates-2026-08-19.md` for the per-model sources,
    /// verbatim prices, and documented inferences). Anything not listed
    /// there stays visibly unpriced.
    pub fn bundled() -> Self {
        let mut dataset = Self {
            source: BUNDLED_SOURCE.to_owned(),
            version: BUNDLED_VERSION.to_owned(),
            rates: BTreeMap::new(),
            aliases: BTreeMap::new(),
        };
        for (key, rates) in SEED_RATES {
            dataset.rates.insert((*key).to_owned(), *rates);
        }
        for (alias, canonical) in SEED_ALIASES {
            dataset
                .aliases
                .insert((*alias).to_owned(), (*canonical).to_owned());
        }
        dataset
    }

    /// Matching precedence: exact provider-qualified match, then exact bare
    /// model match, then reviewed alias. Anything else stays unpriced.
    pub fn lookup(&self, provider: Option<&str>, model: &str) -> Option<RateMatch<'_>> {
        if model.is_empty() {
            return None;
        }
        let exact = provider
            .filter(|provider| !provider.is_empty())
            .map(|provider| format!("{provider}/{model}"))
            .and_then(|qualified| {
                self.rates
                    .get_key_value(qualified.as_str())
                    .map(|(key, _)| key.as_str())
            })
            .or_else(|| self.rates.get_key_value(model).map(|(key, _)| key.as_str()));
        if let Some(key) = exact {
            return Some(RateMatch::Exact { key });
        }
        if let Some((alias, canonical)) = self.aliases.get_key_value(model)
            && let Some((canonical_key, _)) = self.rates.get_key_value(canonical.as_str())
        {
            return Some(RateMatch::Alias {
                alias: alias.as_str(),
                canonical: canonical_key.as_str(),
            });
        }
        None
    }

    pub fn estimate(
        &self,
        provider: Option<&str>,
        model: &str,
        tokens: TokenBreakdown,
    ) -> CostEstimate {
        let provenance = self.provenance();
        match self.lookup(provider, model) {
            Some(RateMatch::Exact { key }) => match self.rates[key].cost(tokens) {
                Some(usd) => CostEstimate {
                    usd: Some(usd),
                    kind: CostKind::ApiEquivalentEstimate,
                    confidence: DataConfidence::Estimated,
                    pricing_key: Some(key.to_owned()),
                    pricing_source: Some(provenance),
                    pricing_rule: Some("exact".to_owned()),
                },
                None => CostEstimate::unpriced(provenance, "overflow"),
            },
            Some(RateMatch::Alias { alias, canonical }) => {
                match self.rates[canonical].cost(tokens) {
                    Some(usd) => CostEstimate {
                        usd: Some(usd),
                        kind: CostKind::ApiEquivalentEstimate,
                        confidence: DataConfidence::Estimated,
                        pricing_key: Some(canonical.to_owned()),
                        pricing_source: Some(provenance),
                        pricing_rule: Some(format!("alias:{alias}")),
                    },
                    None => CostEstimate::unpriced(provenance, "overflow"),
                }
            }
            None => CostEstimate::unpriced(provenance, "no-match"),
        }
    }

    pub fn provenance(&self) -> String {
        format!("{}@{}", self.source, self.version)
    }

    /// Stable content fingerprint for the `pricing_snapshots` ledger: the
    /// same rates, aliases, source, and version always hash identically.
    pub fn content_hash(&self) -> String {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for value in [&self.source, &self.version] {
            for byte in value.as_bytes() {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(0x100_0000_01b3);
            }
            hash ^= 0;
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
        for (key, rates) in &self.rates {
            for part in [
                key.as_bytes(),
                &rates.input.to_le_bytes(),
                &rates.output.to_le_bytes(),
                &rates.cache_read.to_le_bytes(),
                &rates.cache_write.to_le_bytes(),
                &rates.reasoning.to_le_bytes(),
            ] {
                for byte in part {
                    hash ^= u64::from(*byte);
                    hash = hash.wrapping_mul(0x100_0000_01b3);
                }
            }
        }
        for (alias, canonical) in &self.aliases {
            for value in [alias, canonical] {
                for byte in value.as_bytes() {
                    hash ^= u64::from(*byte);
                    hash = hash.wrapping_mul(0x100_0000_01b3);
                }
            }
        }
        format!("{hash:016x}")
    }
}

/// The outcome of pricing one event against a dataset.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CostEstimate {
    pub usd: Option<NanoUsd>,
    pub kind: CostKind,
    pub confidence: DataConfidence,
    pub pricing_key: Option<String>,
    pub pricing_source: Option<String>,
    pub pricing_rule: Option<String>,
}

impl CostEstimate {
    fn unpriced(provenance: String, rule: &str) -> Self {
        Self {
            usd: None,
            kind: CostKind::Unpriced,
            confidence: DataConfidence::Estimated,
            pricing_key: None,
            pricing_source: Some(provenance),
            pricing_rule: Some(format!("unpriced:{rule}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use agentmeter_core::{CostKind, DataConfidence, NanoUsd, TokenBreakdown};

    use super::{BUNDLED_SOURCE, BUNDLED_VERSION, ModelRates, RateDataset};

    fn dataset() -> RateDataset {
        RateDataset {
            source: "test-reviewed".into(),
            version: "1.0".into(),
            rates: [
                (
                    "openai/gpt-test".to_owned(),
                    ModelRates {
                        input: 3_000,
                        output: 12_000,
                        cache_read: 300,
                        cache_write: 3_750,
                        reasoning: 12_000,
                    },
                ),
                (
                    "kimi-for-coding".to_owned(),
                    ModelRates {
                        input: 1_000,
                        ..ModelRates::default()
                    },
                ),
            ]
            .into_iter()
            .collect(),
            aliases: [("kimi-coding".to_owned(), "kimi-for-coding".to_owned())]
                .into_iter()
                .collect(),
        }
    }

    #[test]
    fn computes_exact_integer_estimates_per_bucket() {
        let tokens = TokenBreakdown {
            input: 1_000_000,
            output: 500_000,
            cache_read: 2_000_000,
            cache_write: 100_000,
            reasoning: 50_000,
        };
        let estimate = dataset()
            .estimate(Some("openai"), "gpt-test", tokens)
            .usd
            .unwrap();

        // $3/M input + $12/M output + $0.30/M cache read + $3.75/M write + reasoning at output rate.
        let expected_nanos = 1_000_000 * 3_000
            + 500_000 * 12_000
            + 2_000_000 * 300
            + 100_000 * 3_750
            + 50_000 * 12_000;
        assert_eq!(estimate, NanoUsd::from_nanos(expected_nanos));
    }

    #[test]
    fn provider_qualified_matches_beat_bare_and_alias_matches() {
        let dataset = dataset();
        assert!(matches!(
            dataset.lookup(Some("openai"), "gpt-test"),
            Some(super::RateMatch::Exact {
                key: "openai/gpt-test"
            })
        ));
        assert!(matches!(
            dataset.lookup(None, "kimi-for-coding"),
            Some(super::RateMatch::Exact {
                key: "kimi-for-coding"
            })
        ));
        assert!(matches!(
            dataset.lookup(None, "kimi-coding"),
            Some(super::RateMatch::Alias {
                alias: "kimi-coding",
                canonical: "kimi-for-coding"
            })
        ));
        assert_eq!(dataset.lookup(None, "claude-unknown"), None);
        assert_eq!(dataset.lookup(None, ""), None);
    }

    #[test]
    fn alias_estimates_record_the_alias_rule() {
        let estimate = dataset().estimate(
            None,
            "kimi-coding",
            TokenBreakdown {
                input: 10,
                ..TokenBreakdown::default()
            },
        );

        assert_eq!(estimate.kind, CostKind::ApiEquivalentEstimate);
        assert_eq!(estimate.pricing_key.as_deref(), Some("kimi-for-coding"));
        assert_eq!(estimate.pricing_rule.as_deref(), Some("alias:kimi-coding"));
        assert_eq!(
            estimate.pricing_source.as_deref(),
            Some("test-reviewed@1.0")
        );
        assert_eq!(estimate.confidence, DataConfidence::Estimated);
        assert_eq!(estimate.usd, Some(NanoUsd::from_nanos(10_000)));
    }

    #[test]
    fn unknown_or_overflowing_models_stay_visibly_unpriced() {
        let unknown = dataset().estimate(
            None,
            "totally-unknown",
            TokenBreakdown {
                input: 5,
                ..TokenBreakdown::default()
            },
        );
        assert_eq!(unknown.kind, CostKind::Unpriced);
        assert_eq!(unknown.usd, None);
        assert_eq!(unknown.pricing_rule.as_deref(), Some("unpriced:no-match"));

        let overflowing = dataset().estimate(
            None,
            "kimi-for-coding",
            TokenBreakdown {
                input: u64::MAX,
                ..TokenBreakdown::default()
            },
        );
        assert_eq!(overflowing.kind, CostKind::Unpriced);
        assert_eq!(
            overflowing.pricing_rule.as_deref(),
            Some("unpriced:overflow")
        );
    }

    #[test]
    fn bundled_dataset_is_versioned_and_seed_verified() {
        let bundled = RateDataset::bundled();

        assert_eq!(bundled.source, BUNDLED_SOURCE);
        assert_eq!(bundled.version, BUNDLED_VERSION);
        assert_eq!(
            bundled.rates["gpt-5.3-codex"],
            ModelRates {
                input: 1_750,
                output: 14_000,
                cache_read: 175,
                cache_write: 1_750,
                reasoning: 14_000,
            }
        );
        assert!(bundled.rates.contains_key("claude-opus-5-5"));
        // $56.25 per million cache-write tokens stays integral in nanos.
        assert_eq!(bundled.rates["claude-opus-5-5"].cache_write, 56_250);
        assert!(bundled.rates.contains_key("deepseek-v4-flash"));

        let estimate = bundled.estimate(
            None,
            "kimi-for-coding",
            TokenBreakdown {
                input: 1_000_000,
                ..TokenBreakdown::default()
            },
        );
        assert_eq!(
            estimate.usd,
            Some(NanoUsd::from_nanos(950_000_000)),
            "the CLI alias prices at the official kimi-k2.7-code miss rate"
        );
        assert_eq!(estimate.pricing_key.as_deref(), Some("kimi-k2.7-code"));

        assert_eq!(
            bundled
                .estimate(
                    None,
                    "gpt-5.3-codex-mini",
                    TokenBreakdown {
                        input: 1,
                        ..TokenBreakdown::default()
                    }
                )
                .pricing_rule
                .as_deref(),
            Some("unpriced:no-match"),
            "models absent from official pricing pages stay unpriced"
        );
    }

    #[test]
    fn content_hash_is_stable_and_tracks_content() {
        let first = dataset();
        let second = dataset();
        assert_eq!(first.content_hash(), second.content_hash());

        let mut changed = dataset();
        changed
            .rates
            .entry("kimi-for-coding".to_owned())
            .or_default()
            .input = 2_000;
        assert_ne!(first.content_hash(), changed.content_hash());

        let mut reversioned = dataset();
        reversioned.version = "2.0".into();
        assert_ne!(first.content_hash(), reversioned.content_hash());
    }
}
