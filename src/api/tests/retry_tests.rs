/// Tests for retry strategy functionality
#[cfg(test)]
mod tests {
    use crate::error::{
        retry::{BackoffStrategy, RetryPolicy, RetryStrategy},
        ApiError, ErrorCategory,
    };
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[tokio::test]
    async fn test_retry_policy_should_retry() {
        let policy = RetryPolicy::default();

        // Should retry server errors
        let server_error = ApiError::internal_server_error("Server error", "Details", None);
        assert!(policy.should_retry(&server_error, 0));
        assert!(policy.should_retry(&server_error, 1));
        assert!(!policy.should_retry(&server_error, 3)); // Max attempts reached

        // Should not retry client errors
        let client_error = ApiError::bad_request("Client error", "Details", None);
        assert!(!policy.should_retry(&client_error, 0));
    }

    #[tokio::test]
    async fn test_backoff_strategies() {
        // Fixed backoff
        let fixed = BackoffStrategy::Fixed {
            delay: Duration::from_millis(100),
        };
        assert_eq!(fixed.calculate_delay(0), Duration::from_millis(100));
        assert_eq!(fixed.calculate_delay(5), Duration::from_millis(100));

        // Linear backoff
        let linear = BackoffStrategy::Linear {
            base_delay: Duration::from_millis(100),
            increment: Duration::from_millis(50),
        };
        assert_eq!(linear.calculate_delay(0), Duration::from_millis(100));
        assert_eq!(linear.calculate_delay(1), Duration::from_millis(150));
        assert_eq!(linear.calculate_delay(2), Duration::from_millis(200));

        // Exponential backoff
        let exponential = BackoffStrategy::Exponential {
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            multiplier: 2.0,
        };
        assert_eq!(exponential.calculate_delay(0), Duration::from_millis(100));
        assert_eq!(exponential.calculate_delay(1), Duration::from_millis(200));
        assert_eq!(exponential.calculate_delay(2), Duration::from_millis(400));
    }

    #[tokio::test]
    async fn test_retry_strategy_success_after_failures() {
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
    async fn test_retry_strategy_max_attempts() {
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

    #[tokio::test]
    async fn test_different_retry_policies() {
        // Test external service policy
        let external_policy = RetryPolicy::external_service();
        assert_eq!(external_policy.max_attempts, 5);
        assert!(external_policy
            .retryable_errors
            .contains(&ErrorCategory::External));

        // Test database policy
        let db_policy = RetryPolicy::database();
        assert_eq!(db_policy.max_attempts, 3);
        assert!(db_policy.retryable_errors.contains(&ErrorCategory::Server));

        // Test rate limited policy
        let rate_policy = RetryPolicy::rate_limited();
        assert_eq!(rate_policy.max_attempts, 10);
        assert!(rate_policy
            .retryable_errors
            .contains(&ErrorCategory::RateLimit));
    }
}
