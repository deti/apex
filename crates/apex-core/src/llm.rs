use std::sync::Mutex;

use async_trait::async_trait;

use crate::error::{ApexError, Result};

#[derive(Debug, Clone)]
pub struct LlmMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn complete(&self, messages: &[LlmMessage], max_tokens: u32) -> Result<LlmResponse>;
    fn model_name(&self) -> &str;
}

/// A mock LLM client for testing. Returns pre-queued responses in FIFO order.
pub struct MockLlmClient {
    responses: Mutex<Vec<String>>,
}

impl MockLlmClient {
    pub fn new(responses: Vec<String>) -> Self {
        Self {
            responses: Mutex::new(responses),
        }
    }
}

#[async_trait]
impl LlmClient for MockLlmClient {
    async fn complete(&self, _messages: &[LlmMessage], _max_tokens: u32) -> Result<LlmResponse> {
        let mut queue = self
            .responses
            .lock()
            .map_err(|e| ApexError::Other(format!("mutex poisoned: {e}")))?;
        if queue.is_empty() {
            return Err(ApexError::Agent(
                "MockLlmClient: no more queued responses".into(),
            ));
        }
        let content = queue.remove(0);
        Ok(LlmResponse {
            content,
            input_tokens: 0,
            output_tokens: 0,
        })
    }

    fn model_name(&self) -> &str {
        "mock"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_message_creation_and_clone() {
        let msg = LlmMessage {
            role: "user".into(),
            content: "hello".into(),
        };
        let cloned = msg.clone();
        assert_eq!(cloned.role, "user");
        assert_eq!(cloned.content, "hello");
    }

    #[test]
    fn llm_response_creation() {
        let resp = LlmResponse {
            content: "hi".into(),
            input_tokens: 10,
            output_tokens: 5,
        };
        assert_eq!(resp.content, "hi");
        assert_eq!(resp.input_tokens, 10);
    }

    #[tokio::test]
    async fn mock_returns_queued_responses() {
        let mock = MockLlmClient::new(vec!["response one".into(), "response two".into()]);
        let msgs = [LlmMessage {
            role: "user".into(),
            content: "hi".into(),
        }];
        let r1 = mock.complete(&msgs, 100).await.unwrap();
        assert_eq!(r1.content, "response one");
        let r2 = mock.complete(&msgs, 100).await.unwrap();
        assert_eq!(r2.content, "response two");
    }

    #[tokio::test]
    async fn mock_returns_error_when_empty() {
        let mock = MockLlmClient::new(vec![]);
        let msgs = [LlmMessage {
            role: "user".into(),
            content: "hi".into(),
        }];
        assert!(mock.complete(&msgs, 100).await.is_err());
    }

    #[test]
    fn mock_model_name() {
        let mock = MockLlmClient::new(vec![]);
        assert_eq!(mock.model_name(), "mock");
    }
}
