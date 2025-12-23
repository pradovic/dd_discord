mod common;
use axum::extract::State;
use axum::Json;
use common::create_test_db;
use common::DropDb;
use dd_discord::db::Action;
use dd_discord::db::CustomID;
use dd_discord::util;
use http::StatusCode;
use serde_json::json;
use twilight_model::channel::message::MessageFlags;
use twilight_model::http::interaction::InteractionResponse;
use twilight_model::http::interaction::InteractionResponseData;

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use dd_discord::{handle_interaction, InteractionError};
use ddclient_rs::Voting;
use ed25519_dalek::{Signer as _, SigningKey};
use httpmock::{Method::POST, MockServer};
use rand::rngs::OsRng;

macro_rules! create_mock {
    ($server:expr, $method:expr, $path:expr, $body:expr) => {{
        let mock = $server.mock(|when, then| {
            when.method($method).path($path);
            then.status(200)
                .header("Content-Type", "application/json")
                .json_body($body);
        });
        mock
    }};
}

macro_rules! run_test {
    ($assertion_name:expr,$test:expr,$dd_mocks:expr,$discord_mocks:expr,$expected_result:expr, $assert_mocks:expr) => {{
        let mut mocks = Vec::new();
        for mock in $dd_mocks {
            mocks.push(create_mock!($test.dd_server, mock.0, mock.1, mock.2));
        }

        for mock in $discord_mocks {
            mocks.push(create_mock!($test.discord_server, mock.0, mock.1, mock.2));
        }

        let resp = handle_interaction(
            $test.data.clone(),
            $test.headers.clone(),
            $test.body.to_string(),
        )
        .await;

        match (&resp, &$expected_result) {
            (Ok((status_l, json_l)), Ok((status_r, json_r))) => {
                assert_eq!(status_l, status_r);
                assert_eq!(json_l.0, json_r.0);
            }

            (Err(err_l), Err(err_r)) => {
                assert_eq!(err_l, err_r);
            }

            _ => panic!("expected {:?} got {:?}", $assertion_name, resp),
        }

        if $assert_mocks {
            for mock in mocks.iter_mut() {
                mock.assert();
                mock.delete();
            }
        }

        mocks
    }};
}

#[tokio::test]
async fn handle_interaction_uknown_command() {
    let test = setup_test_env("unknown_comman.json");
    run_test!(
        "unknown command",
        &test,
        empty_mock_vec(),
        empty_mock_vec(),
        internal_server_error_response(),
        true
    );
}

#[tokio::test]
async fn handle_interaction_bad_signature() {
    let test = setup_test_env("slash_command.json");

    let mut headers = http::HeaderMap::new();
    headers.insert("X-Signature-Ed25519", "bad signature".parse().unwrap());
    headers.insert("X-Signature-Timestamp", "bad timestamp".parse().unwrap());
    let resp = handle_interaction(test.data.clone(), headers, test.body.clone()).await;

    if matches!(resp, Err(InteractionError::Status(StatusCode::UNAUTHORIZED))) {
    } else {
        panic!("expected Unauthorized got {resp:?}");
    }
}

#[tokio::test]
#[expect(clippy::too_many_lines, reason = "Integration test with comprehensive setup and assertions")]
async fn handle_slash_interaction() {
    let test = setup_test_env("slash_command.json");

    let voting = Voting {
        id: "4712947128794".to_owned(),
        choices: vec![
            "Spinoza".to_owned(),
            "Kant".to_owned(),
            "Nietzsche".to_owned(),
        ], // from slash_command.json
    };

    let dm_channel_id = "319674150115610528";
    let user_id = "82198898841029460";
    let channel_id = "1187315505103638638"; // from slash_command.json
    let creator_message_id = "812746127846424";
    let message_id = "3589723985723";

    let dd_client_happy_mocks = || -> Vec<(httpmock::Method, String, serde_json::Value)> {
        vec![(
            POST,
            "/v1/votings".to_owned(),
            serde_json::json!(&voting),
        )]
    };

    let discord_client_happy_mocks = || -> Vec<(httpmock::Method, String, serde_json::Value)> {
        vec![
            (
                POST,
                "/api/v10/users/@me/channels".to_owned(),
                json!({
                  "id": dm_channel_id,
                  "type": 1,
                  "last_message_id": null,
                  "recipients": [
                    {
                      "username": "test",
                      "discriminator": "9999",
                      "id": user_id,
                      "avatar": "33ecab261d4681afa4d85a04691c4a01"
                    }
                  ],
                  "application_id": null
                }),
            ),
            (
                POST,
                format!("/api/v10/channels/{dm_channel_id}/messages"),
                json!({
                            "attachments": [],
                            "author": {
                              "username": "test",
                              "discriminator": "9999",
                              "id": user_id,
                              "avatar": "33ecab261d4681afa4d85a04691c4a01"
                            },
                            "channel_id": dm_channel_id,
                            "content": "test",
                            "edited_timestamp": null,
                            "embeds": [],
                            "flags": 0,
                            "id": creator_message_id,
                            "mention_everyone": false,
                            "mention_roles": [],
                            "mentions": [],
                            "pinned": false,
                            "timestamp": "2018-02-04T19:51:45.941000+00:00",
                            "tts": false,
                            "type": 0
                }),
            ),
            (
                POST,
                format!("/api/v10/channels/{channel_id}/messages"),
                json!({
                            "attachments": [],
                            "author": {
                              "username": "test",
                              "discriminator": "9999",
                              "id": user_id,
                              "avatar": "33ecab261d4681afa4d85a04691c4a01"
                            },
                            "channel_id": channel_id,
                            "content": "test",
                            "edited_timestamp": null,
                            "embeds": [],
                            "flags": 0,
                            "id": message_id,
                            "mention_everyone": false,
                            "mention_roles": [],
                            "mentions": [],
                            "pinned": false,
                            "timestamp": "2018-02-04T19:51:45.941000+00:00",
                            "tts": false,
                            "type": 0
                }),
            ),
        ]
    };

    run_test!(
      "happy path",
      &test,
     dd_client_happy_mocks(),
     discord_client_happy_mocks(),
      Ok((StatusCode::OK, Json(InteractionResponse{
        kind: twilight_model::http::interaction::InteractionResponseType::ChannelMessageWithSource,
        data: None,
      }))), true);

    let expected_voting = dd_discord::db::Voting {
        id: voting.id.clone(),
        choices: voting.choices.clone(),
        channel_id: channel_id.to_owned(),
        message_id: message_id.to_owned(),
        name: "Who do you prefer?".to_owned(), // from slash_command.json
        is_completed: false,
        is_deleted: false,
        creator_message_id: creator_message_id.to_owned(),
        creator_dm_channel_id: dm_channel_id.to_owned(),
    };

    let got_voting = test.data.db.get_voting(&voting.id).await.unwrap();
    assert_eq!(got_voting, expected_voting);

    let custom_ids = test.data.db.get_custom_ids(&voting.id).await.unwrap();
    assert_eq!(custom_ids.len(), 3);

    run_test!(
        "dd client create voting error",
        &test,
        [(
            POST,
            "/v1/votings".to_owned(),
            json!({
              "error": "error",
            })
        )],
        empty_mock_vec(),
        internal_server_error_response(),
        true
    );

    run_test!(
        "discord client create private channel error",
        &test,
        dd_client_happy_mocks(),
        [(
            POST,
            "/api/v10/users/@me/channels",
            json!({
              "error": "error",
            })
        )],
        internal_server_error_response(),
        true
    );

    run_test!(
        "discord client create dm message error",
        &test,
        dd_client_happy_mocks(),
        [
            discord_client_happy_mocks().swap_remove(0),
            (
                POST,
                "/api/v10/channels/319674150115610528/messages".to_owned(),
                json!({
                  "error": "error",
                })
            )
        ],
        internal_server_error_response(),
        true
    );

    run_test!(
        "discord client create channel message error",
        &test,
        dd_client_happy_mocks(),
        [
            discord_client_happy_mocks().swap_remove(0),
            discord_client_happy_mocks().swap_remove(1),
            (
                POST,
                format!("/api/v10/channels/{channel_id}/messages"),
                json!({
                  "error": "error",
                })
            )
        ],
        internal_server_error_response(),
        true
    );
}

const fn empty_mock_vec() -> Vec<(httpmock::Method, &'static str, serde_json::Value)> {
    vec![]
}

#[tokio::test]
#[expect(clippy::too_many_lines, reason = "Integration test with comprehensive setup and assertions")]
async fn handle_vote_channel_test() {
    let custom_uuid = "df4db2bc-9fd1-43fb-8e17-97170379159a";
    let dm_channel_id = "319674150115610528";
    let user_id = "82198898841029460"; // vote_channel.json
    let creator_message_id = "812746127846424";

    let voting = dd_discord::db::Voting {
        id: "4712947128794".to_owned(),
        choices: vec![
            "Spinoza".to_owned(),
            "Kant".to_owned(),
            "Nietzsche".to_owned(),
        ],
        channel_id: "1187315505103638638".to_owned(),
        message_id: "3589723985723".to_owned(),
        name: "Who do you prefer?".to_owned(),
        is_completed: false,
        is_deleted: false,
        creator_message_id: creator_message_id.to_owned(),
        creator_dm_channel_id: dm_channel_id.to_owned(),
    };

    let test = setup_test_env("vote_channel.json");
    test.data
        .db
        .save_voting(voting.clone())
        .await
        .expect("Failed to save voting");
    test.data
        .db
        .bulk_save_custom_ids(vec![
            (
                util::generate_random_custom_uuid(),
                CustomID {
                    action: Action::Complete,
                    voting_id: voting.id.clone(),
                    user_id: None,
                    page: None,
                    index: None,
                },
            ),
            (
                util::generate_random_custom_uuid(),
                CustomID {
                    action: Action::Delete,
                    voting_id: voting.id.clone(),
                    user_id: None,
                    page: None,
                    index: None,
                },
            ),
            (
                custom_uuid.to_owned(),
                CustomID {
                    action: Action::VoteFromChannel,
                    voting_id: voting.id.clone(),
                    user_id: None,
                    page: None,
                    index: None,
                },
            ),
        ])
        .await
        .expect("Failed to save custom ids");

    let discord_client_happy_mocks = || -> Vec<(httpmock::Method, String, serde_json::Value)> {
        vec![
            (
                POST,
                "/api/v10/users/@me/channels".to_owned(),
                json!({
                  "id": dm_channel_id,
                  "type": 1,
                  "last_message_id": null,
                  "recipients": [
                    {
                      "username": "test",
                      "discriminator": "9999",
                      "id": user_id,
                      "avatar": "33ecab261d4681afa4d85a04691c4a01"
                    }
                  ],
                  "application_id": null
                }),
            ),
            (
                POST,
                format!("/api/v10/channels/{dm_channel_id}/messages"),
                json!({
                            "attachments": [],
                            "author": {
                              "username": "test",
                              "discriminator": "9999",
                              "id": user_id,
                              "avatar": "33ecab261d4681afa4d85a04691c4a01"
                            },
                            "channel_id": dm_channel_id,
                            "content": "test",
                            "edited_timestamp": null,
                            "embeds": [],
                            "flags": 0,
                            "id": creator_message_id,
                            "mention_everyone": false,
                            "mention_roles": [],
                            "mentions": [],
                            "pinned": false,
                            "timestamp": "2018-02-04T19:51:45.941000+00:00",
                            "tts": false,
                            "type": 0
                }),
            ),
        ]
    };

    let mocks = run_test!(
      "happy path",
      &test,
     empty_mock_vec(),
     discord_client_happy_mocks(),
      Ok((StatusCode::OK, Json(InteractionResponse{
        kind: twilight_model::http::interaction::InteractionResponseType::ChannelMessageWithSource,
        data: Some(InteractionResponseData {
            content: Some("You will receive dm with voting dialog".to_owned()),
            flags: Some(MessageFlags::EPHEMERAL),
            ..Default::default()
        }),
      }))),
    false);

    let start = tokio::time::Instant::now();
    let timeout_duration = Duration::from_secs(5);

    let voting_dialog = loop {
        if let Ok(voting_dialog) = test.data.db.get_voting_dialog(&voting.id, user_id).await { break voting_dialog }
        assert!((start.elapsed() <= timeout_duration), "get voting dialog timeout");

        tokio::time::sleep(Duration::from_millis(100)).await;
    };

    assert_eq!(voting_dialog.voting_id, voting.id);

    for mut mock in mocks {
        mock.assert();
        mock.delete();
    }

    let custom_ids = test.data.db.get_custom_ids(&voting.id).await.unwrap();
    assert_eq!(custom_ids.len(), 7);
}

fn create_dd_client_server() -> (MockServer, ddclient_rs::Client) {
    let mock_server = MockServer::start();
    let dd_client = ddclient_rs::Client::builder("dd_token".to_owned())
        .api_url(mock_server.base_url())
        .build();

    (mock_server, dd_client)
}

fn create_discord_client_server() -> (MockServer, twilight_http::Client) {
    let mock_server = MockServer::start();
    let base_url = mock_server.base_url().replace("http://", "");
    let discord_client = twilight_http::Client::builder()
        .token("bot_token".to_owned())
        .proxy(base_url, true)
        .build();

    (mock_server, discord_client)
}

struct TestEnvironment {
    #[expect(dead_code, reason = "DropDb must be held to prevent cleanup until test ends")]
    drop_db: DropDb,
    dd_server: MockServer,
    discord_server: MockServer,
    body: String,
    data: State<Arc<dd_discord::AppState>>,
    headers: http::HeaderMap,
}

fn setup_test_env(filename: &str) -> TestEnvironment {
    let filename = format!("{}/{}", "tests/data", filename);
    let body = fs::read_to_string(filename).expect("Failed to read file");
    let (drop_db, db) = create_test_db();
    let (dd_server, dd_client) = create_dd_client_server();
    let (discord_server, discord_client) = create_discord_client_server();

    let (headers, discord_public_key) = signing_headers(&body);
    let app_state = State(dd_discord::new_app_state(
        db,
        discord_client,
        dd_client,
        discord_public_key,
    ));

    TestEnvironment {
        drop_db,
        dd_server,
        discord_server,
        body,
        data: app_state,
        headers,
    }
}

fn signing_headers(body: &str) -> (http::HeaderMap, String) {
    let mut csprng = OsRng;
    let signing_key: SigningKey = SigningKey::generate(&mut csprng);

    let timestamp = "timestamp".to_owned();
    let mut signing_buff = timestamp.as_bytes().to_vec();
    signing_buff.extend_from_slice(body.as_bytes());

    let signature = signing_key.sign(&signing_buff);
    let signature = signature.to_bytes();
    let signature = hex::encode(signature);
    let public_key = hex::encode(signing_key.verifying_key().as_bytes());

    let mut headers = http::HeaderMap::new();
    headers.insert("X-Signature-Ed25519", signature.parse().unwrap());
    headers.insert("X-Signature-Timestamp", timestamp.parse().unwrap());

    (headers, public_key)
}

const fn internal_server_error_response() -> dd_discord::InteractionResult {
    Err(InteractionError::InternalServerError)
}

// this can be used for debugging tests
#[expect(dead_code, reason = "Debug helper function kept for test troubleshooting")]
fn setup_tracing() {
    tracing_subscriber::fmt()
        .with_test_writer()
        .with_env_filter("httpmock=debug")
        .init();
}
