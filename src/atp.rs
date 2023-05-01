use std::fmt::Display;

use anyhow::Result;
use async_recursion::async_recursion;
use reqwest::{Request, Response, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use time::format_description::well_known::Iso8601;
use time::OffsetDateTime;

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiError {
    error: String,
    message: Option<String>,
}

impl Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "Error: {}\nMessage: {}",
            self.error,
            self.message.clone().unwrap_or_default()
        )
    }
}

#[derive(Debug, Error)]
pub enum XrpcError {
    #[error("XRPC API Error\n{0}")]
    API(ApiError),
    #[error("Rate limited")]
    RateLimited,
    #[error("Internal XRPC Client Error '{0}'")]
    Internal(&'static str),
}

type XrpcResult<T> = Result<T, XrpcError>;

#[derive(Debug)]
pub struct XrpcAuth {
    access_token: String,
    refresh_token: String,
    did: String,
}

#[derive(Debug)]
pub struct XrpcClient {
    provider: String,
    http: reqwest::Client,
    auth: Option<XrpcAuth>,
}

impl XrpcClient {
    fn xrpc(&self, method: &str) -> String {
        format!("{}/xrpc/{}", self.provider, method)
    }

    pub async fn new(provider: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            http: reqwest::Client::new(),
            auth: None,
        }
    }

    pub(crate) async fn query<I, O>(&mut self, method: &str, input: Option<I>) -> XrpcResult<O>
    where
        I: Serialize,
        O: DeserializeOwned,
    {
        let url = self.xrpc(method);
        let mut builder = self.http.get(url);

        if let Some(input) = input {
            builder = builder.query(&input);
        }

        if let Some(auth) = &self.auth {
            builder = builder.bearer_auth(&auth.access_token);
        }

        let request = builder
            .build()
            .map_err(|_| XrpcError::Internal("Failed to build query request"))?;

        let response = self
            .make_request(request, true)
            .await?
            .json::<O>()
            .await
            .map_err(|_| XrpcError::Internal("Failed to get query response"))?;

        Ok(response)
    }

    pub(crate) async fn procedure<I>(
        &mut self,
        method: &str,
        input: Option<I>,
    ) -> XrpcResult<Response>
    where
        I: Serialize,
    {
        let url = self.xrpc(method);
        let mut builder = self.http.post(url);

        if let Some(input) = input {
            builder = builder.json(&input);
        }

        if let Some(auth) = &self.auth {
            builder = builder.bearer_auth(&auth.access_token);
        }

        let request = builder
            .build()
            .map_err(|_| XrpcError::Internal("Failed to build procedure request"))?;

        let response = self.make_request(request, true).await?;

        Ok(response)
    }

    pub(crate) async fn procedure_io<I, O>(
        &mut self,
        method: &str,
        input: Option<I>,
    ) -> XrpcResult<O>
    where
        I: Serialize,
        O: DeserializeOwned,
    {
        let response = self
            .procedure(method, input)
            .await?
            .json::<O>()
            .await
            .map_err(|_| XrpcError::Internal("Failed to get procedure response"))?;

        Ok(response)
    }

    #[async_recursion(?Send)]
    async fn make_request(&mut self, request: Request, retry: bool) -> XrpcResult<Response> {
        let response = self
            .http
            .execute(request.try_clone().expect("Request should be clonable"))
            .await
            .map_err(|_| XrpcError::Internal("Failed to execute request"))?;

        // If the response failed we find out the reason
        if response.status() != StatusCode::OK {
            let error = response
                .json::<ApiError>()
                .await
                .map_err(|_| XrpcError::Internal("Failed to parse api error"))?;

            // Return early if the error isn't an expired token, value of retry is false or
            // if the client isn't authenticated to begin with.
            if "ExpiredToken" != &error.error || !retry || self.auth.is_none() {
                return Err(XrpcError::API(error));
            }

            self.refresh_auth().await?;
            return self.make_request(request, false).await;
        }

        Ok(response)
    }

    pub async fn login(
        &mut self,
        handle: impl Into<String>,
        password: impl Into<String>,
    ) -> XrpcResult<()> {
        let body = CreateSessionRequest {
            identifier: handle.into(),
            password: password.into(),
        };

        let session = self
            .procedure_io::<_, SessionResponse>("com.atproto.server.createSession", Some(body))
            .await?;

        self.auth = Some(XrpcAuth {
            access_token: session.access_jwt,
            refresh_token: session.refresh_jwt,
            did: session.did,
        });

        Ok(())
    }

    pub async fn refresh_auth(&mut self) -> XrpcResult<()> {
        let Some(auth) = &self.auth else {
            return Ok(());
        };

        let url = self.xrpc("com.atproto.server.refreshSession");
        let response = self
            .http
            .post(url)
            .bearer_auth(&auth.refresh_token)
            .send()
            .await
            .map_err(|_| XrpcError::Internal("Failed to build session refresh request"))?;

        if response.status() != StatusCode::OK {
            let error = response
                .json::<ApiError>()
                .await
                .map_err(|_| XrpcError::Internal("Failed to parse api error"))?;

            return Err(XrpcError::API(error));
        }

        let response = response
            .json::<SessionResponse>()
            .await
            .map_err(|_| XrpcError::Internal("Failed to get session refresh response"))?;

        self.auth = Some(XrpcAuth {
            access_token: response.access_jwt,
            refresh_token: response.refresh_jwt,
            did: response.did,
        });

        Ok(())
    }

    pub async fn get_post_thread(
        &mut self,
        input: GetPostThreadParams,
    ) -> XrpcResult<GetPostThread> {
        let post_thread = self
            .query("app.bsky.feed.getPostThread", Some(input))
            .await?;

        Ok(post_thread)
    }

    pub async fn post_reply(
        &mut self,
        parent_uri: impl Into<String>,
        parent_cid: impl Into<String>,
        contents: impl Into<String>,
    ) -> XrpcResult<String> {
        let Some(auth) = &self.auth else {
            return Err(XrpcError::Internal("Endpoint requires authentication"));
        };

        let parent_uri = parent_uri.into();
        let parent_cid = parent_cid.into();

        let now = OffsetDateTime::now_utc()
            .format(&Iso8601::DEFAULT)
            .map_err(|_| XrpcError::Internal("Failed creating datetime"))?;

        let input = json!({
            "collection": "app.bsky.feed.post",
            "repo": auth.did,
            "record": {
                "$type": "app.bsky.feed.post",
                "createdAt": now,
                "reply": {
                    "parent": {
                        "uri": parent_uri.clone(),
                        "cid": parent_cid.clone(),
                    },
                    "root": {
                        "uri": parent_uri,
                        "cid": parent_cid,
                    }
                },
                "text": contents.into(),
            }
        });

        let response = self
            .procedure_io::<_, Value>("com.atproto.repo.createRecord", Some(input))
            .await?;

        Ok(response
            .get("uri")
            .ok_or(XrpcError::Internal("Could not get uri"))?
            .to_string())
    }

    pub async fn list_notifications(&mut self) -> XrpcResult<ListNotifications> {
        let notifications = self
            .query::<(), _>("app.bsky.notification.listNotifications", None)
            .await?;

        Ok(notifications)
    }

    pub async fn seen_notifications(&mut self, moment: String) -> XrpcResult<()> {
        let input = json!({ "seenAt": moment });

        self.procedure::<_>("app.bsky.notification.updateSeen", Some(input))
            .await?;

        Ok(())
    }
}

// TODO: vvvv USE AUTOMATIC LEXICON GENERATION IN FUUUUUTURE vvvv

// Auth
// =

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveHandleResponse {
    pub did: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionRequest {
    pub identifier: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResponse {
    pub access_jwt: String,
    pub refresh_jwt: String,
    pub handle: String,
    pub did: String,
    pub email: Option<String>,
}

// Post Thread
// =

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetPostThreadParams {
    pub uri: String,
    pub depth: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetPostThread {
    pub thread: ThreadView,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadView {
    pub post: Option<PostView>,
    pub parent: Option<Box<ThreadView>>,
    #[serde(default)]
    pub replies: Vec<ThreadView>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostView {
    pub uri: String,
    pub cid: String,
    pub author: PostAuthor,
    pub record: Record,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostAuthor {
    pub did: String,
    pub handle: String,
}

// Notifications
// =

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListNotifications {
    pub notifications: Vec<Notification>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub uri: String,
    pub cid: String,
    pub author: Value,
    #[serde(flatten)]
    pub reason: NotificationReason,
    pub record: Record,
    pub is_read: bool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "reason", rename_all = "camelCase")]
pub enum NotificationReason {
    Like,
    Repost,
    Follow,
    Mention,
    Reply,
    Quote,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    // FIXME: Contents of record depends on type, make this an enum
    pub text: Option<String>,
    #[serde(rename = "$type")]
    pub typ: String,
}
