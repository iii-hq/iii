use opentelemetry::trace::{Link, SamplingDecision, SamplingResult, SpanKind, TraceId};
use opentelemetry::{Context, KeyValue};
use opentelemetry_sdk::trace::ShouldSample;
use regex::Regex;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::config::{RateLimitConfig, SamplingRule};

/// Token bucket for rate limiting trace sampling.
///
/// Uses atomic operations for lock-free token consumption and periodic refills.
#[derive(Debug)]
pub struct TokenBucket {
    max_tokens: u32,
    /// Fixed-point representation: actual_tokens * 1000
    tokens: AtomicU64,
    last_refill: Mutex<Instant>,
    refill_rate: u32, // Tokens per second
}

impl TokenBucket {
    pub fn new(max_traces_per_second: u32) -> Self {
        let initial_tokens = (max_traces_per_second as u64) * 1000;
        Self {
            max_tokens: max_traces_per_second,
            tokens: AtomicU64::new(initial_tokens),
            last_refill: Mutex::new(Instant::now()),
            refill_rate: max_traces_per_second,
        }
    }

    /// Try to consume one token. Returns true if successful.
    pub fn try_consume(&self) -> bool {
        // Refill tokens based on elapsed time
        self.refill();

        // Try to consume one token atomically
        loop {
            let current = self.tokens.load(Ordering::Relaxed);
            if current < 1000 {
                // Not enough tokens (less than 1.0)
                return false;
            }

            let new_value = current - 1000;
            if self
                .tokens
                .compare_exchange(current, new_value, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
            {
                return true;
            }
            // CAS failed, retry
        }
    }

    fn refill(&self) {
        if let Ok(mut last_refill) = self.last_refill.try_lock() {
            let now = Instant::now();
            let elapsed = now.duration_since(*last_refill);

            if elapsed >= Duration::from_millis(100) {
                // Refill tokens based on elapsed time
                let tokens_to_add =
                    (elapsed.as_secs_f64() * self.refill_rate as f64 * 1000.0) as u64;

                if tokens_to_add > 0 {
                    let max_tokens = (self.max_tokens as u64) * 1000;
                    loop {
                        let current = self.tokens.load(Ordering::Relaxed);
                        let new_value = (current + tokens_to_add).min(max_tokens);

                        if self
                            .tokens
                            .compare_exchange(
                                current,
                                new_value,
                                Ordering::SeqCst,
                                Ordering::Relaxed,
                            )
                            .is_ok()
                        {
                            break;
                        }
                    }

                    *last_refill = now;
                }
            }
        }
    }
}

impl Clone for TokenBucket {
    fn clone(&self) -> Self {
        // When cloning, create a new bucket with reset state
        Self::new(self.max_tokens)
    }
}

/// Single compiled sampling rule with precompiled regex patterns.
#[derive(Debug, Clone)]
pub struct CompiledRule {
    operation_pattern: Option<Regex>,
    service_pattern: Option<Regex>,
    rate: f64,
}

impl CompiledRule {
    pub fn new(rule: &SamplingRule) -> Result<Self, regex::Error> {
        let operation_pattern = if let Some(ref pattern) = rule.operation {
            Some(Self::wildcard_to_regex(pattern)?)
        } else {
            None
        };

        let service_pattern = if let Some(ref pattern) = rule.service {
            Some(Self::wildcard_to_regex(pattern)?)
        } else {
            None
        };

        Ok(Self {
            operation_pattern,
            service_pattern,
            rate: rule.rate,
        })
    }

    /// Convert a wildcard pattern to a regex pattern.
    /// Escapes all regex metacharacters first, then converts wildcards:
    /// - `*` becomes `.*` (match any characters)
    /// - `?` becomes `.` (match single character)
    fn wildcard_to_regex(pattern: &str) -> Result<Regex, regex::Error> {
        // First escape all regex metacharacters
        let escaped = regex::escape(pattern);
        // Then convert our wildcard tokens back:
        // regex::escape turns `*` into `\*` and `?` into `\?`
        let regex_pattern = escaped
            .replace(r"\*", ".*")
            .replace(r"\?", ".");
        Regex::new(&format!("^{}$", regex_pattern))
    }

    /// Check if this rule matches the given span.
    pub fn matches(&self, operation_name: &str, service_name: Option<&str>) -> bool {
        // Check operation pattern
        if let Some(ref pattern) = self.operation_pattern
            && !pattern.is_match(operation_name)
        {
            return false;
        }

        // Check service pattern
        if let Some(ref pattern) = self.service_pattern {
            match service_name {
                Some(svc) => {
                    if !pattern.is_match(svc) {
                        return false;
                    }
                }
                None => return false, // Rule requires service but none provided
            }
        }

        true
    }
}

/// Advanced sampler with per-operation rules and rate limiting.
///
/// This sampler supports:
/// - Per-operation sampling rates (e.g., different rates for "api.*" vs "health.*")
/// - Per-service sampling rates (e.g., different rates for "worker-*" vs "api-*")
/// - Rate limiting (max traces per second)
/// - Fallback to default sampling ratio
#[derive(Debug, Clone)]
pub struct AdvancedSampler {
    default_ratio: f64,
    rules: Vec<CompiledRule>,
    rate_limiter: Option<TokenBucket>,
    service_name: Option<String>,
}

impl AdvancedSampler {
    pub fn new(
        default_ratio: f64,
        rules: Vec<SamplingRule>,
        rate_limit_config: Option<RateLimitConfig>,
        service_name: Option<String>,
    ) -> Result<Self, regex::Error> {
        let compiled_rules: Result<Vec<_>, _> = rules.iter().map(CompiledRule::new).collect();

        let rate_limiter = rate_limit_config.map(|cfg| TokenBucket::new(cfg.max_traces_per_second));

        Ok(Self {
            default_ratio,
            rules: compiled_rules?,
            rate_limiter,
            service_name,
        })
    }

    fn get_sampling_ratio(&self, operation_name: &str) -> f64 {
        // Find first matching rule
        for rule in &self.rules {
            if rule.matches(operation_name, self.service_name.as_deref()) {
                return rule.rate;
            }
        }

        // No rule matched, use default
        self.default_ratio
    }

    fn should_sample_by_ratio(&self, trace_id: TraceId, ratio: f64) -> bool {
        if ratio >= 1.0 {
            return true;
        }
        if ratio <= 0.0 {
            return false;
        }

        // Use trace ID for deterministic sampling
        // This is similar to TraceIdRatioBased sampler
        let trace_id_bytes = trace_id.to_bytes();
        let mut value: u64 = 0;
        for (i, &byte) in trace_id_bytes.iter().take(8).enumerate() {
            value |= (byte as u64) << (i * 8);
        }

        // Convert to 0.0-1.0 range
        let threshold = (ratio * (u64::MAX as f64)) as u64;
        value < threshold
    }
}

impl ShouldSample for AdvancedSampler {
    fn should_sample(
        &self,
        _parent_context: Option<&Context>,
        trace_id: TraceId,
        name: &str,
        _span_kind: &SpanKind,
        _attributes: &[KeyValue],
        _links: &[Link],
    ) -> SamplingResult {
        // 1. Check rate limit first (if configured)
        if let Some(ref limiter) = self.rate_limiter
            && !limiter.try_consume()
        {
            // Rate limit exceeded, drop the trace
            return SamplingResult {
                decision: SamplingDecision::Drop,
                attributes: Vec::new(),
                trace_state: Default::default(),
            };
        }

        // 2. Find matching rule and get sampling ratio
        let ratio = self.get_sampling_ratio(name);

        // 3. Make sampling decision based on trace ID and ratio
        let should_sample = self.should_sample_by_ratio(trace_id, ratio);

        let decision = if should_sample {
            SamplingDecision::RecordAndSample
        } else {
            SamplingDecision::Drop
        };

        SamplingResult {
            decision,
            attributes: Vec::new(),
            trace_state: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_bucket_consume() {
        let bucket = TokenBucket::new(10);

        // Should be able to consume up to max tokens
        for _ in 0..10 {
            assert!(bucket.try_consume());
        }

        // Should fail when tokens exhausted
        assert!(!bucket.try_consume());
    }

    #[test]
    fn test_token_bucket_refill() {
        let bucket = TokenBucket::new(100); // 100 tokens per second

        // Consume all tokens
        for _ in 0..100 {
            bucket.try_consume();
        }

        assert!(!bucket.try_consume());

        // Wait and refill
        std::thread::sleep(Duration::from_millis(200));

        // Should have refilled ~20 tokens (100 tokens/sec * 0.2 sec)
        assert!(bucket.try_consume());
    }

    #[test]
    fn test_compiled_rule_matching() {
        let rule = SamplingRule {
            operation: Some("api.*".to_string()),
            service: None,
            rate: 0.5,
        };

        let compiled = CompiledRule::new(&rule).unwrap();

        assert!(compiled.matches("api.users", None));
        assert!(compiled.matches("api.orders", None));
        assert!(!compiled.matches("health.check", None));
        assert!(!compiled.matches("apixusers", None));
    }

    #[test]
    fn test_advanced_sampler_rule_matching() {
        let rules = vec![
            SamplingRule {
                operation: Some("health.*".to_string()),
                service: None,
                rate: 0.01,
            },
            SamplingRule {
                operation: Some("api.*".to_string()),
                service: None,
                rate: 0.5,
            },
        ];

        let sampler = AdvancedSampler::new(0.1, rules, None, None).unwrap();

        // Should match specific rules
        assert_eq!(sampler.get_sampling_ratio("health.check"), 0.01);
        assert_eq!(sampler.get_sampling_ratio("api.users"), 0.5);

        // Should fall back to default
        assert_eq!(sampler.get_sampling_ratio("other.operation"), 0.1);
    }

    #[test]
    fn test_advanced_sampler_default_ratio() {
        let sampler = AdvancedSampler::new(0.1, vec![], None, None).unwrap();

        assert_eq!(sampler.get_sampling_ratio("any.operation"), 0.1);
    }

    #[test]
    fn test_advanced_sampler_rate_limiting() {
        let rate_limit = RateLimitConfig {
            max_traces_per_second: 5,
        };

        let sampler = AdvancedSampler::new(1.0, vec![], Some(rate_limit), None).unwrap();

        // First 5 should succeed
        let mut sampled = 0;
        for i in 0..10 {
            let trace_id = TraceId::from_bytes([i as u8; 16]);
            let result = sampler.should_sample(
                None,
                trace_id,
                "test.operation",
                &SpanKind::Internal,
                &[],
                &[],
            );

            if matches!(result.decision, SamplingDecision::RecordAndSample) {
                sampled += 1;
            }
        }

        // Should have sampled at most 5 (rate limit)
        assert!(sampled <= 5, "Sampled {} traces, expected <= 5", sampled);
    }

    #[test]
    fn test_compiled_rule_service_matching() {
        // Test service-only rule
        let rule = SamplingRule {
            operation: None,
            service: Some("worker-*".to_string()),
            rate: 0.25,
        };

        let compiled = CompiledRule::new(&rule).unwrap();

        // Should match when service matches pattern
        assert!(compiled.matches("any.operation", Some("worker-1")));
        assert!(compiled.matches("any.operation", Some("worker-abc")));

        // Should not match when service doesn't match pattern
        assert!(!compiled.matches("any.operation", Some("api-gateway")));
        assert!(!compiled.matches("any.operation", Some("workerx"))); // no dash

        // Should not match when no service provided but rule requires it
        assert!(!compiled.matches("any.operation", None));
    }

    #[test]
    fn test_compiled_rule_combined_matching() {
        // Test combined operation + service rule
        let rule = SamplingRule {
            operation: Some("api.*".to_string()),
            service: Some("production-*".to_string()),
            rate: 0.8,
        };

        let compiled = CompiledRule::new(&rule).unwrap();

        // Both must match
        assert!(compiled.matches("api.users", Some("production-east")));
        assert!(compiled.matches("api.orders", Some("production-west")));

        // Operation matches but service doesn't
        assert!(!compiled.matches("api.users", Some("staging-east")));

        // Service matches but operation doesn't
        assert!(!compiled.matches("health.check", Some("production-east")));

        // Neither matches
        assert!(!compiled.matches("health.check", Some("staging-east")));

        // No service provided
        assert!(!compiled.matches("api.users", None));
    }

    #[test]
    fn test_advanced_sampler_service_rule_matching() {
        let rules = vec![
            SamplingRule {
                operation: Some("api.*".to_string()),
                service: Some("production-*".to_string()),
                rate: 0.9, // High sampling for production API
            },
            SamplingRule {
                operation: Some("api.*".to_string()),
                service: Some("staging-*".to_string()),
                rate: 0.1, // Low sampling for staging API
            },
            SamplingRule {
                operation: Some("api.*".to_string()),
                service: None, // Fallback for API without service match
                rate: 0.5,
            },
        ];

        // Create sampler with production service
        let prod_sampler = AdvancedSampler::new(
            0.1,
            rules.clone(),
            None,
            Some("production-east".to_string()),
        )
        .unwrap();
        assert_eq!(prod_sampler.get_sampling_ratio("api.users"), 0.9);

        // Create sampler with staging service
        let staging_sampler =
            AdvancedSampler::new(0.1, rules.clone(), None, Some("staging-west".to_string()))
                .unwrap();
        assert_eq!(staging_sampler.get_sampling_ratio("api.users"), 0.1);

        // Create sampler with unknown service - falls through to service=None rule
        let dev_sampler =
            AdvancedSampler::new(0.1, rules.clone(), None, Some("dev-local".to_string())).unwrap();
        assert_eq!(dev_sampler.get_sampling_ratio("api.users"), 0.5);

        // Non-API operations should use default ratio
        assert_eq!(prod_sampler.get_sampling_ratio("health.check"), 0.1);
    }

    #[test]
    fn test_advanced_sampler_service_only_rule() {
        let rules = vec![SamplingRule {
            operation: None, // Match any operation
            service: Some("critical-*".to_string()),
            rate: 1.0, // Always sample critical services
        }];

        let sampler =
            AdvancedSampler::new(0.1, rules, None, Some("critical-payments".to_string())).unwrap();

        // Any operation from critical service should be sampled at 100%
        assert_eq!(sampler.get_sampling_ratio("api.users"), 1.0);
        assert_eq!(sampler.get_sampling_ratio("health.check"), 1.0);
        assert_eq!(sampler.get_sampling_ratio("any.operation"), 1.0);
    }

    #[test]
    fn test_compiled_rule_metacharacter_escaping() {
        // Pattern with regex metacharacters that should be treated literally
        let rule = SamplingRule {
            operation: Some("api.v1+beta[test]".to_string()),
            service: None,
            rate: 0.5,
        };

        let compiled = CompiledRule::new(&rule).unwrap();

        // Should match the literal string
        assert!(compiled.matches("api.v1+beta[test]", None));
        // Should NOT match variations that would match if metacharacters weren't escaped
        assert!(!compiled.matches("api.v11beta[test]", None)); // + not treated as "one or more"
        assert!(!compiled.matches("apixv1+betattest]", None)); // . not treated as "any char"
    }
}
