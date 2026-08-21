//! 落雪 OAuth 的授权与 token HTTP 适配器。
//!
//! subject 绑定、state 生命周期、持久化与刷新并发控制不属于这一层。

mod authorization;
mod client;
mod config;
mod error;
mod pkce;
mod redaction;
mod tokens;

const MAX_TOKEN_RESPONSE_BYTES: usize = 1024 * 1024;

pub use authorization::AuthorizationRequest;
pub use client::LxnsOAuthClient;
pub use config::{DEFAULT_AUTHORIZE_URL, DEFAULT_SCOPES, DEFAULT_TOKEN_URL, OAuthConfig};
pub use error::{OAuthError, OAuthErrorCode};
pub use pkce::{OAuthState, PkceVerifier};
pub use tokens::OAuthTokens;

#[cfg(test)]
mod tests;
