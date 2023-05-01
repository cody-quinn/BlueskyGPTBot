pub mod atp;

use std::env;
use std::time::Duration;

use anyhow::Result;
use atp::{GetPostThreadParams, PostView, XrpcClient};
use openai::chat::{ChatCompletion, ChatCompletionMessage, ChatCompletionMessageRole};
use time::format_description::well_known::Iso8601;
use time::OffsetDateTime;
use tracing::{event, Level};

use crate::atp::NotificationReason;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv()?;
    tracing_subscriber::fmt()
        .with_target(false)
        .with_timer(tracing_subscriber::fmt::time::uptime())
        .with_level(true)
        .init();

    let bs_provider = env::var("BLUESKY_PROVIDER")?;
    let bs_handle = env::var("BLUESKY_HANDLE")?;
    let bs_password = env::var("BLUESKY_PASSWORD")?;

    let openai_key = env::var("OPENAI_KEY")?;

    // Setting the OpenAI key for the client
    // TODO: Switch clients, this is awful
    openai::set_key(openai_key);

    // Logging into our client
    let mut client = XrpcClient::new(&bs_provider).await;
    client.login(&bs_handle, &bs_password).await?;
    event!(Level::INFO, "Logged into BlueSky as '{bs_handle}'");

    // TODO: Run this stuff on multiple threads. This requires make the client
    // capable of being shared accross multiple threads however.
    //
    // Poll for events on a loop
    let mut interval = tokio::time::interval(Duration::from_secs(20));

    loop {
        interval.tick().await;

        let Ok(events) = poll_events(&mut client).await else {
            event!(Level::ERROR, "Failed to poll events");
            continue;
        };

        for event in events.into_iter() {
            let result = process_request(&mut client, event).await;

            if let Err(e) = result {
                event!(Level::ERROR, "Failed to respond to event: {}", e);
            }
        }
    }
}

#[derive(Debug)]
struct BotRequest {
    uri: String,
}

#[derive(Debug)]
enum BotRequestResult {
    Success,
    InvalidRequest,
}

async fn poll_events(client: &mut XrpcClient) -> Result<Vec<BotRequest>> {
    // Getting the instant we will use to read our notifications
    let now = OffsetDateTime::now_utc().format(&Iso8601::DEFAULT)?;

    // Getting all notifications that are mentions and haven't been read
    let notifs = client.list_notifications().await?;
    let notifs = notifs
        .notifications
        .into_iter()
        .filter(|it| it.reason == NotificationReason::Mention && !it.is_read)
        .map(|it| BotRequest { uri: it.uri })
        .collect::<Vec<_>>();

    // Marking all the unread notifications as read
    client.seen_notifications(now).await?;

    event!(Level::INFO, "Polling notifications, {} found", notifs.len());

    Ok(notifs)
}

async fn process_request(client: &mut XrpcClient, request: BotRequest) -> Result<BotRequestResult> {
    event!(Level::INFO, "Processing request for {}", request.uri);

    let thread = client
        .get_post_thread(GetPostThreadParams {
            uri: request.uri,
            depth: Some(0),
        })
        .await?
        .thread;

    let Some(child) = thread.post else {
        event!(Level::WARN, "Invalid request. Child post not found");
        return Ok(BotRequestResult::InvalidRequest);
    };

    let Some(parent) = thread.parent.and_then(|it| it.post) else {
        event!(Level::WARN, "Invalid request. Parent post not found");
        return Ok(BotRequestResult::InvalidRequest);
    };

    let Some(response) = generate_response(&parent).await? else {
        return Ok(BotRequestResult::InvalidRequest);
    };

    let mut response = response.chars().take(280).collect::<String>();
    response.push_str("\n\n🤖 info in bio");

    let reply = client.post_reply(child.uri, child.cid, response).await?;

    event!(
        Level::INFO,
        "Fulfilled request for {}.\nURI: {}",
        child.author.handle,
        reply
    );

    Ok(BotRequestResult::Success)
}

async fn generate_response(post: &PostView) -> Result<Option<String>> {
    let system = include_str!("system.txt");
    let Some(user) = &post.record.text else {
        return Ok(None);
    };

    let prompt = format!("@{}\n{}", post.author.handle, user);

    let chat = ChatCompletion::builder("gpt-3.5-turbo-0301", [
        ChatCompletionMessage {
            role: ChatCompletionMessageRole::System,
            content: system.to_owned(),
            name: None,
        },
        ChatCompletionMessage {
            role: ChatCompletionMessageRole::User,
            content: prompt,
            name: None,
        },
    ])
    .user(post.author.did.to_owned())
    .max_tokens(80u32)
    .temperature(0.7);

    let completion = chat.create().await??;
    let Some(response) = completion.choices.first() else {
        return Ok(None);
    };
    let message = &response.message.content;

    event!(
        Level::INFO,
        "Spent {} tokens generating response of length {} to @{}\n\"{}\"",
        completion
            .usage
            .map(|it| it.total_tokens)
            .unwrap_or_default(),
        message.len(),
        post.author.handle,
        message,
    );

    Ok(Some(message.to_owned()))
}
