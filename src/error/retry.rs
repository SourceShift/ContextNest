use super::types::{ApiError, ErrorCategory};
/// Robust retry strategies for transient failures
use crate::error::ContextNestResult;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::{sleep, Instant};
use tracing::{debug, error, warn};

/// Retry policy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts
    pub max_attempts: u32,
    /// Backoff strategy to use
    pub backoff_strategy: BackoffStrategy,
    /// Maximum total retry duration
    pub max_total_duration: Duration,
    /// Which errors should trigger retries
    pub retryable_errors: Vec<ErrorCategory>,
    /// Additional conditions for retry
    pub custom_retry_condition: Option<String>,
}

impl RetryPolicy {
    /// Create a default retry policy
    pub fn default() -> Self {
        Self {
            max_attempts: 3,
            backoff_strategy: BackoffStrategy::ExponentialWithJitter {
                base_delay: Duration::from_millis(1000),
                max_delay: Duration::from_secs(30),
                multiplier: 2.0,
                jitter_factor: 0.1,
            },
            max_total_duration: Duration::from_secs(300), // 5 minutes
            retryable_errors: vec![
                ErrorCategory::Server,
                ErrorCategory::External,
                ErrorCategory::Network,
                ErrorCategory::RateLimit,
            ],
            custom_retry_condition: None,
        }
    }

    /// Create a policy for external service calls
    pub fn external_service() -> Self {
        Self {
            max_attempts: 5,
            backoff_strategy: BackoffStrategy::ExponentialWithJitter {
                base_delay: Duration::from_millis(500),
                max_delay: Duration::from_secs(60),
                multiplier: 1.5,
                jitter_factor: 0.2,
            },
            max_total_duration: Duration::from_secs(180), // 3 minutes
            retryable_errors: vec![
                ErrorCategory::External,
                ErrorCategory::Network,
                ErrorCategory::RateLimit,
            ],
            custom_retry_condition: None,
        }
    }

    /// Create a policy for database operations
    pub fn database() -> Self {
        Self {
            max_attempts: 3,
            backoff_strategy: BackoffStrategy::Linear {
                base_delay: Duration::from_millis(2000),
                increment: Duration::from_millis(1000),
            },
            max_total_duration: Duration::from_secs(30),
            retryable_errors: vec![ErrorCategory::Server, ErrorCategory::Network],
            custom_retry_condition: None,
        }
    }

    /// Create a policy for rate-limited operations
    pub fn rate_limited() -> Self {
        Self {
            max_attempts: 10,
            backoff_strategy: BackoffStrategy::Fixed {
                delay: Duration::from_millis(1000),
            },
            max_total_duration: Duration::from_secs(60),
            retryable_errors: vec![ErrorCategory::RateLimit],
            custom_retry_condition: None,
        }
    }

    /// Check if an error should be retried
    pub fn should_retry(&self, error: &ApiError, attempt: u32) -> bool {
        // Check attempt limit
        if attempt >= self.max_attempts {
            return false;
        }

        // Check if error category is retryable
        if !self.retryable_errors.contains(&error.category) {
            return false;
        }

        // Check explicit retryable flag
        if !error.retryable {
            return false;
        }

        // Additional checks for specific error types
        match error.category {
            ErrorCategory::RateLimit => true,
            ErrorCategory::Server => error.status_code >= 500,
            ErrorCategory::External => true,
            ErrorCategory::Network => true,
            _ => false,
        }
    }

    /// Calculate delay for the next retry attempt
    pub fn calculate_delay(&self, attempt: u32) -> Duration {
        self.backoff_strategy.calculate_delay(attempt)
    }
}

/// Backoff strategy for retry delays
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BackoffStrategy {
    /// Fixed delay between retries
    Fixed { delay: Duration },
    /// Linear increase in delay
    Linear {
        base_delay: Duration,
        increment: Duration,
    },
    /// Exponential backoff
    Exponential {
        base_delay: Duration,
        max_delay: Duration,
        multiplier: f64,
    },
    /// Exponential backoff with jitter to avoid thundering herd
    ExponentialWithJitter {
        base_delay: Duration,
        max_delay: Duration,
        multiplier: f64,
        jitter_factor: f64,
    },
    /// Custom delays for each attempt
    Custom { delays: Vec<Duration> },
}

impl BackoffStrategy {
    /// Calculate delay for a specific attempt
    pub fn calculate_delay(&self, attempt: u32) -> Duration {
        match self {
            BackoffStrategy::Fixed { delay } => *delay,

            BackoffStrategy::Linear {
                base_delay,
                increment,
            } => *base_delay + *increment * attempt,

            BackoffStrategy::Exponential {
                base_delay,
                max_delay,
                multiplier,
            } => {
                let delay = base_delay.as_millis() as f64 * multiplier.powi(attempt as i32);
                Duration::from_millis((delay as u64).min(max_delay.as_millis() as u64))
            }

            BackoffStrategy::ExponentialWithJitter {
                base_delay,
                max_delay,
                multiplier,
                jitter_factor,
            } => {
                let base_delay_ms = base_delay.as_millis() as f64;
                let exponential_delay = base_delay_ms * multiplier.powi(attempt as i32);

                // Add jitter to prevent thundering herd
                let jitter =
                    exponential_delay * jitter_factor * (rand::random::<f64>() - 0.5) * 2.0;
                let final_delay = exponential_delay + jitter;

                Duration::from_millis(
                    (final_delay as u64)
                        .max(0)
                        .min(max_delay.as_millis() as u64),
                )
            }

            BackoffStrategy::Custom { delays } => delays
                .get(attempt as usize)
                .copied()
                .unwrap_or_else(|| delays.last().copied().unwrap_or(Duration::from_secs(1))),
        }
    }
}

/// Retry strategy executor
#[derive(Debug)]
pub struct RetryStrategy {
    policy: RetryPolicy,
}

impl RetryStrategy {
    /// Create a new retry strategy with the given policy
    pub fn new(policy: RetryPolicy) -> Self {
        Self { policy }
    }

    /// Create a retry strategy with default policy
    pub fn default() -> Self {
        Self::new(RetryPolicy::default())
    }

    /// Execute a function with retry logic
    pub async fn execute<F, T, E>(&self, mut operation: F) -> std::result::Result<T, ApiError>
    where
        F: FnMut() -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<T, E>> + Send + 'static>,
            > + Send,
        E: Into<ApiError> + Send,
    {
        let start_time = Instant::now();
        let mut attempt = 0;
        let mut last_error: Option<ApiError> = None;

        loop {
            // Check if we've exceeded the maximum total duration
            if start_time.elapsed() > self.policy.max_total_duration {
                warn!(
                    "Retry strategy exceeded maximum total duration of {:?}",
                    self.policy.max_total_duration
                );
                break;
            }

            debug!("Executing operation attempt {}", attempt + 1);

            match operation().await {
                Ok(result) => {
                    if attempt > 0 {
                        debug!("Operation succeeded after {} retries", attempt);
                    }
                    return Ok(result);
                }
                Err(error) => {
                    let api_error: ApiError = error.into();
                    last_error = Some(api_error.clone());

                    if !self.policy.should_retry(&api_error, attempt) {
                        debug!(
                            "Not retrying error: category={:?}, attempt={}, retryable={}",
                            api_error.category, attempt, api_error.retryable
                        );
                        break;
                    }

                    attempt += 1;

                    if attempt >= self.policy.max_attempts {
                        warn!(
                            "Maximum retry attempts ({}) reached for operation",
                            self.policy.max_attempts
                        );
                        break;
                    }

                    let delay = self.policy.calculate_delay(attempt - 1);
                    debug!(
                        "Retrying operation in {:?} (attempt {} of {})",
                        delay,
                        attempt + 1,
                        self.policy.max_attempts
                    );

                    sleep(delay).await;
                }
            }
        }

        // Return the last error if all retries failed
        Err(last_error.unwrap_or_else(|| {
            ApiError::internal_server_error(
                "Retry strategy failed",
                "Operation failed after all retry attempts",
                None,
            )
        }))
    }

    /// Execute a simple async function with retry logic
    pub async fn execute_simple<F, T>(&self, mut operation: F) -> std::result::Result<T, ApiError>
    where
        F: FnMut() -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<T, ApiError>> + Send + 'static>,
            > + Send,
    {
        self.execute(|| operation()).await
    }
}

/// Retry executor for specific operation types
pub struct RetryExecutor;

impl RetryExecutor {
    /// Execute database operation with retry
    pub async fn database<F, T>(operation: F) -> std::result::Result<T, ApiError>
    where
        F: Fn() -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<T, ApiError>> + Send + 'static>,
            > + Send,
    {
        let strategy = RetryStrategy::new(RetryPolicy::database());
        strategy.execute(operation).await
    }

    /// Execute external service call with retry
    pub async fn external_service<F, T>(operation: F) -> std::result::Result<T, ApiError>
    where
        F: Fn() -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<T, ApiError>> + Send + 'static>,
            > + Send,
    {
        let strategy = RetryStrategy::new(RetryPolicy::external_service());
        strategy.execute(operation).await
    }

    /// Execute rate-limited operation with retry
    pub async fn rate_limited<F, T>(operation: F) -> std::result::Result<T, ApiError>
    where
        F: Fn() -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<T, ApiError>> + Send + 'static>,
            > + Send,
    {
        let strategy = RetryStrategy::new(RetryPolicy::rate_limited());
        strategy.execute(operation).await
    }

    /// Execute with custom retry policy
    pub async fn with_policy<F, T>(
        policy: RetryPolicy,
        operation: F,
    ) -> std::result::Result<T, ApiError>
    where
        F: Fn() -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<T, ApiError>> + Send + 'static>,
            > + Send,
    {
        let strategy = RetryStrategy::new(policy);
        strategy.execute(operation).await
    }
}

/// Helper macro for creating retry operations
#[macro_export]
macro_rules! retry_operation {
    ($policy:expr, $operation:expr) => {
        $crate::error::retry::RetryExecutor::with_policy($policy, || {
            Box::pin(async { $operation.await })
        })
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_retry_success_after_failures() {
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = Arc::clone(&counter);

        let policy = RetryPolicy {
            max_attempts: 3,
            backoff_strategy: BackoffStrategy::Fixed {
                delay: Duration::from_millis(10),
            },
            max_total_duration: Duration::from_secs(10),
            retryable_errors: vec![ErrorCategory::Server],
            custom_retry_condition: None,
        };

        let strategy = RetryStrategy::new(policy);

        let result = strategy
            .execute(|| {
                let counter = Arc::clone(&counter_clone);
                Box::pin(async move {
                    let count = counter.fetch_add(1, Ordering::SeqCst);
                    if count < 2 {
                        Err(ApiError::internal_server_error("test error", "test", None))
                    } else {
                        Ok("success")
                    }
                })
            })
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_retry_max_attempts_exceeded() {
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = Arc::clone(&counter);

        let policy = RetryPolicy {
            max_attempts: 2,
            backoff_strategy: BackoffStrategy::Fixed {
                delay: Duration::from_millis(10),
            },
            max_total_duration: Duration::from_secs(10),
            retryable_errors: vec![ErrorCategory::Server],
            custom_retry_condition: None,
        };

        let strategy = RetryStrategy::new(policy);

        let result: Result<(), ApiError> = strategy
            .execute(|| {
                let counter = Arc::clone(&counter_clone);
                Box::pin(async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Err(ApiError::internal_server_error("test error", "test", None))
                })
            })
            .await;

        assert!(result.is_err());
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }
}
