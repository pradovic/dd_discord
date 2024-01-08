pub mod db;
pub mod util;

use crate::db::{Action, CustomID, Db, Voting};

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::Json;
use ddclient_rs::Client;
use http::{HeaderMap, StatusCode};
use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::task::TaskTracker;
use twilight_model::application::interaction::application_command::{
    CommandData, CommandOptionValue,
};
use twilight_model::application::interaction::message_component::MessageComponentInteractionData;
use twilight_model::application::interaction::{Interaction, InteractionData, InteractionType};
use twilight_model::channel::message::component::{
    ActionRow, Button, ButtonStyle, Component, SelectMenuOption,
};
use twilight_model::channel::message::{Embed, MessageFlags};
use twilight_model::channel::Message;
use twilight_model::http::interaction::{
    InteractionResponse, InteractionResponseData, InteractionResponseType,
};
use twilight_model::id::marker::{ChannelMarker, MessageMarker};
use twilight_model::id::Id;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder};

pub type InteractionResult = Result<(StatusCode, Json<InteractionResponse>), InteractionError>;

pub struct AppState {
    pub db: Db,
    pub discord_client: twilight_http::Client,
    pub dd_client: Client,
    pub discord_public_key: String,
    pub task_tracker: TaskTracker,
}

pub fn new_app_state(
    db: Db,
    discord_client: twilight_http::Client,
    dd_client: Client,
    discord_public_key: String,
) -> Arc<AppState> {
    Arc::new(AppState {
        db,
        discord_client,
        dd_client,
        discord_public_key,
        task_tracker: TaskTracker::new(),
    })
}

pub async fn handle_interaction(
    State(data): State<Arc<AppState>>,
    headers: HeaderMap,
    body: String,
) -> InteractionResult {
    let interaction: Interaction = serde_json::from_str(&body).map_err(|err| {
        tracing::error!(error = ?err, "parsing interaction from body failed");
        InteractionError::Status(StatusCode::BAD_REQUEST)
    })?;

    tracing::debug!(?interaction, "received interaction");
    util::verify_signature(headers, body, &data.discord_public_key).map_err(|err| {
        tracing::error!(error = ?err,"verifying signature failed");
        InteractionError::Status(StatusCode::UNAUTHORIZED)
    })?;

    match interaction.kind {
        // this is a ping sent by discord
        InteractionType::Ping => Ok((
            StatusCode::OK,
            Json(InteractionResponse {
                kind: InteractionResponseType::Pong,
                data: None,
            }),
        )),

        InteractionType::ApplicationCommand => {
            let Some(InteractionData::ApplicationCommand(ref command)) = interaction.data else {
                tracing::error!(data = ?interaction.data, "application command data not found");
                return Err(InteractionError::InternalServerError);
            };

            match command.name.as_str() {
                "ping" => handle_ping(),
                "voting" => handle_slash_voting(&data, command, &interaction).await,
                _ => {
                    tracing::error!(data = ?interaction.data, "Application command not handled");
                    Err(InteractionError::InternalServerError)
                }
            }
        }

        InteractionType::MessageComponent => {
            let Some(InteractionData::MessageComponent(command)) = &interaction.data else {
                tracing::error!(data = ?interaction.data, "message component data not found");
                return Err(InteractionError::InternalServerError);
            };

            let Ok(custom_id) = data.db.get_custom_id(&command.custom_id).await else {
                // this can happen with lingering dialogs while completing or deleting voting
                tracing::info!(data = ?interaction.data, "received interaction with unknown custom id");
                return ack_response();
            };

            match &custom_id.action {
                Action::VoteFromChannel => {
                    handle_vote_channel(&data, &interaction, &custom_id.voting_id).await
                }
                Action::VoteFromDM => {
                    handle_dm_vote(&data, &interaction, &custom_id.voting_id).await
                }
                Action::VoteSelect => {
                    handle_vote_select(&data, &interaction, command, &custom_id).await
                }
                Action::VoteNext => handle_vote_page(data, &interaction, &custom_id).await,
                Action::VotePrevious => handle_vote_page(data, &interaction, &custom_id).await,
                Action::Complete => {
                    handle_complete_voting(&data, &interaction, &custom_id.voting_id).await
                }
                Action::Delete => {
                    handle_delete_voting(&data, &interaction, &custom_id.voting_id).await
                }
            }
        }

        _ => {
            tracing::error!(data = ?interaction.data, "Interaction type not handled");
            Err(InteractionError::InternalServerError)
        }
    }
}

async fn handle_vote_page(
    data: Arc<AppState>,
    interaction: &Interaction,
    custom_id: &CustomID,
) -> InteractionResult {
    let voting_id = &custom_id.voting_id;
    let Some(page) = custom_id.page else {
        tracing::error!(%voting_id, data = ?interaction.data, "page not found");
        return Err(InteractionError::InternalServerError);
    };

    let voting = data.db.get_voting(voting_id).await.map_err(|err| {
        tracing::error!(%voting_id, error = ?err, data = ?interaction.data, "db get voting failed");
        InteractionError::InternalServerError
    })?;

    // this can happen with lingering dialogs while completing or deleting voting
    if voting.is_deleted || voting.is_completed {
        return ack_response();
    }

    let Some(ref user) = interaction.user else {
        tracing::error!(%voting_id, data = ?interaction.data, "interaction user not found");
        return Err(InteractionError::InternalServerError);
    };

    let voting_dialog = match data
        .db
        .get_voting_dialog(voting_id, &user.id.to_string())
        .await
    {
        Ok(v) => v,
        Err(db::DbError::NotFound) => {
            return ack_response();
        }
        Err(err) => {
            tracing::error!(%voting_id, error = ?err, data = ?interaction.data, "getting voting dialog from db failed");
            return Err(InteractionError::InternalServerError);
        }
    };

    let (title, components, custom_ids) =
        create_vote_components(voting_id, voting, page, voting_dialog.ballot);
    data.db
        .bulk_save_custom_ids(custom_ids)
        .await
        .map_err(|err| {
            tracing::error!(%voting_id, error = ?err, data = ?interaction.data, "bulk saving custom ids into db failed");
            InteractionError::InternalServerError
        })?;

    let Some(ref channel) = interaction.channel else {
        tracing::error!(%voting_id, data = ?interaction.data, "interaction channel not found");
        return Err(InteractionError::InternalServerError);
    };

    let Some(ref message) = interaction.message else {
        tracing::error!(%voting_id, data = ?interaction.data, "interaction message not found");
        return Err(InteractionError::InternalServerError);
    };

    update_message(
        &data.discord_client,
        channel.id,
        message.id,
        None,
        Some(&title),
        Some(&components),
    )
    .await?;

    ack_response()
}

async fn handle_complete_voting(
    data: &Arc<AppState>,
    interaction: &Interaction,
    voting_id: &str,
) -> InteractionResult {
    let results = data
        .dd_client
        .get_voting_results_duels(voting_id)
        .await
        .map_err(|err| {
            tracing::error!(%voting_id, error = ?err, data = ?interaction.data, "getting voting results duels failed");
            InteractionError::InternalServerError
        })?;

    let voting = match data.db.complete_voting(voting_id).await {
        Ok(v) => v,
        Err(db::DbError::NotFound) => {
            // this can happen during delete
            return ack_response();
        }
        Err(err) => {
            tracing::error!(%voting_id, error = ?err, data = ?interaction.data, "completing voting in db failed");
            return Err(InteractionError::InternalServerError);
        }
    };

    let description = if results.tie {
        "Its a tie!"
    } else {
        "Voting results were calculated using Shultze method. The users are ranked by winning percentages."
    };

    let mut fields = Vec::new();
    for result in &results.results {
        let field_text = format!(
            "Wins: {}, Percentage: {:.2}%",
            result.wins, result.percentage
        );
        fields.push(EmbedFieldBuilder::new(&result.choice, field_text).build());
    }

    let mut result_embed = EmbedBuilder::new()
        .title(voting.name.clone())
        .description(description);

    for field in fields {
        result_embed = result_embed.field(field);
    }

    let mut result_embeds = vec![result_embed.build()];

    if let Some(duels) = results.duels {
        if !duels.is_empty() && !results.tie {
            let mut duels_fields = Vec::new();
            for duel in duels {
                let message;
                if duel.left.strength == duel.right.strength {
                    message = format!(
                        "**{}** and **{}** are tied",
                        duel.left.choice, duel.right.choice
                    );
                } else {
                    let left;
                    let right;
                    if duel.left.strength > duel.right.strength {
                        left = duel.left;
                        right = duel.right;
                    } else {
                        left = duel.right;
                        right = duel.left;
                    }

                    message = format!(
                        "**{}** defeats **{}** by ({} - {}) = {} votes",
                        left.choice,
                        right.choice,
                        left.strength,
                        right.strength,
                        left.strength - right.strength
                    );
                }
                duels_fields.push(EmbedFieldBuilder::new("", &message).build());
            }

            let mut duels_embed = EmbedBuilder::new().title("Result breakdown");

            for field in duels_fields {
                duels_embed = duels_embed.field(field);
            }

            result_embeds.push(duels_embed.build());
        }
    }

    let message_id = Id::new(
        voting
            .message_id
            .parse::<u64>()
            .map_err(|err| {
                tracing::error!(%voting_id, error = ?err, data = ?interaction.data, "parsing message id failed");
                InteractionError::InternalServerError
            })?
    );

    let channel_id = Id::new(
        voting
            .channel_id
            .parse::<u64>()
            .map_err(|err| {
                tracing::error!(%voting_id, error = ?err, data = ?interaction.data, "parsing channel id failed");
                InteractionError::InternalServerError
            })?
    );

    update_message(
        &data.discord_client,
        channel_id,
        message_id,
        Some("Voting completed!"),
        Some(&result_embeds),
        Some(&Vec::new()),
    )
    .await?;

    // update dm creator to "voting completed"
    let creator_dm_channel_id = Id::new(
        voting
            .creator_dm_channel_id
            .parse::<u64>()
            .map_err(|err| {
                tracing::error!(%voting_id, error = ?err, data = ?interaction.data, "parsing dm channel id failed");
                InteractionError::InternalServerError
            })?

    );
    let creator_message_id = Id::new(
        voting
            .creator_message_id
            .parse::<u64>()
            .map_err(|err| {
                tracing::error!(%voting_id, error = ?err, data = ?interaction.data, "parsing creator message id failed");
                InteractionError::InternalServerError
            })?
    );

    update_message(
        &data.discord_client,
        creator_dm_channel_id,
        creator_message_id,
        Some("Voting completed!"),
        Some(&Vec::new()),
        Some(&Vec::new()),
    )
    .await?;

    let data_clone = data.clone();
    spawn_clean_voting_dialogs(voting, data_clone, "Voting completed".to_string());

    ack_response()
}

async fn handle_delete_voting(
    data: &Arc<AppState>,
    interaction: &Interaction,
    voting_id: &str,
) -> InteractionResult {
    let voting = match data.db.delete_voting(voting_id).await {
        Ok(v) => v,
        Err(db::DbError::NotFound) => {
            // handle double click or complete already in progress
            return ack_response();
        }
        Err(err) => {
            tracing::error!(%voting_id, error = ?err, data = ?interaction.data, "deleting voting from db failed");
            return Err(InteractionError::InternalServerError);
        }
    };

    let message_id = Id::new(
        voting
            .message_id
            .parse::<u64>()
            .map_err(|err| {
                tracing::error!(%voting_id, error = ?err, data = ?interaction.data, "parsing message id failed");
                InteractionError::InternalServerError
            })?
    );
    let channel_id = Id::new(
        voting
            .channel_id
            .parse::<u64>()
            .map_err(|err| {
                tracing::error!(%voting_id, error = ?err, data = ?interaction.data, "parsing channel id failed");
                InteractionError::InternalServerError
            })?
    );

    update_message(
        &data.discord_client,
        channel_id,
        message_id,
        Some(format!("Voting deleted: {}", voting.name).as_str()),
        Some(&Vec::new()),
        Some(&Vec::new()),
    )
    .await?;

    let creator_dm_channel_id = Id::new(
        voting
            .creator_dm_channel_id
            .parse::<u64>()
            .map_err(|err| {
                tracing::error!(%voting_id, error = ?err, data = ?interaction.data, "parsing dm channel id failed");
                InteractionError::InternalServerError
            })?
    );
    let creator_message_id = Id::new(
        voting
            .creator_message_id
            .parse::<u64>()
            .map_err(|err| {
                tracing::error!(%voting_id, error = ?err, data = ?interaction.data, "parsing creator message id failed");
                InteractionError::InternalServerError
            })?
    );

    update_message(
        &data.discord_client,
        creator_dm_channel_id,
        creator_message_id,
        Some(format!("Voting deleted: {}", voting.name).as_str()),
        Some(&Vec::new()),
        Some(&Vec::new()),
    )
    .await?;

    let data_clone = data.clone();
    spawn_clean_voting_dialogs(voting, data_clone, "Voting deleted".to_string());

    ack_response()
}

fn spawn_clean_voting_dialogs(voting: Voting, data_clone: Arc<AppState>, message: String) {
    let data = data_clone.clone();
    data.task_tracker.spawn(async move {
        if let Ok(dialogs) = data_clone.db.get_voting_dialogs(voting.id.as_str()).await {
            for dialog in dialogs {
                let Ok(dm_channel_id) = dialog.channel_id.parse::<u64>() else {
                    tracing::error!(%voting.id, "parsing dm channel id failed");
                    continue;
                };

                let Ok(message_id) = dialog.message_id.parse::<u64>() else {
                    tracing::error!(%voting.id, "parsing message id failed");
                    continue;
                };

                if let Err(err) = update_message(
                    &data_clone.discord_client,
                    Id::new(dm_channel_id),
                    Id::new(message_id),
                    Some(format!("{}: {}", message, voting.name).as_str()),
                    Some(&Vec::new()),
                    Some(&Vec::new()),
                )
                .await
                {
                    tracing::error!(error = ?err, "updating message failed");
                    continue;
                }

                if let Err(err) = data_clone
                    .db
                    .delete_voting_dialog(&dialog.voting_id, &dialog.user_id)
                    .await
                {
                    tracing::error!(error = ?err, "deleting voting dialog from db failed")
                }
            }
        }

        if let Err(err) = data_clone.db.delete_custom_ids(&voting.id).await {
            tracing::debug!("deleting custom ids from db failed: {:?}", err);
        }
    });
}

async fn handle_dm_vote(
    data: &Arc<AppState>,
    interaction: &Interaction,
    voting_id: &str,
) -> InteractionResult {
    let Some(ref user_id) = interaction.user else {
        tracing::error!(%voting_id, data = ?interaction.data, "user id not found");
        return Err(InteractionError::InternalServerError);
    };

    let voting = data.db.get_voting(voting_id).await.map_err(|err| {
        tracing::error!(%voting_id, error = ?err, data = ?interaction.data, "db get voting failed");
        InteractionError::InternalServerError
    })?;

    // this can happen with lingering dialogs while completing or deleting voting
    if voting.is_deleted || voting.is_completed {
        return ack_response();
    }

    let voting_dialog = match data
        .db
        .get_voting_dialog(voting_id, &user_id.id.to_string())
        .await
    {
        Ok(v) => v,
        Err(db::DbError::NotFound) => {
            return ack_response();
        }
        Err(err) => {
            tracing::error!(%voting_id, error = ?err, data = ?interaction.data, "getting voting dialog from db failed");
            return Err(InteractionError::InternalServerError);
        }
    };

    let mut ballot = HashMap::new();

    // todo: test this ordering
    for (name, value) in voting.choices.iter().zip(voting_dialog.ballot.iter()) {
        ballot.insert(name.clone(), *value);
    }

    data.dd_client
        .vote(voting_id, &user_id.id.to_string(), ballot)
        .await
        .map_err(|err| {
            tracing::error!(%voting_id, error = ?err, data = ?interaction.data, "voting failed");
            InteractionError::InternalServerError
        })?;

    let Some(ref channel) = interaction.channel else {
        tracing::error!(%voting_id, data = ?interaction.data, "channel not found");
        return Err(InteractionError::InternalServerError);
    };

    let Some(ref message) = interaction.message else {
        tracing::error!(%voting_id, data = ?interaction.data, "message not found");
        return Err(InteractionError::InternalServerError);
    };

    update_message(
        &data.discord_client,
        channel.id,
        message.id,
        Some("Thank you for voting! Your vote has been successfully submitted."),
        Some(&Vec::new()),
        Some(&Vec::new()),
    )
    .await?;

    data.db
                .delete_voting_dialog(voting_id, &user_id.id.to_string())
                .await
                .map_err(|err| {
                    tracing::error!(%voting_id, error = ?err, data = ?interaction.data, "deleting voting dialog from db failed");
                    InteractionError::InternalServerError
                })?;

    ack_response()
}

async fn handle_vote_select(
    data: &Arc<AppState>,
    interaction: &Interaction,
    command: &MessageComponentInteractionData,
    custom_id: &CustomID,
) -> InteractionResult {
    let voting_id = &custom_id.voting_id;
    let Some(index) = custom_id.index else {
        tracing::error!(%voting_id, data = ?interaction.data, "index not found");
        return Err(InteractionError::InternalServerError);
    };

    let Some(ref user_id) = interaction.user else {
        tracing::error!(%voting_id, data = ?interaction.data, "user id not found");
        return Err(InteractionError::InternalServerError);
    };

    let Some(vote) = command.values.first() else {
        tracing::error!(%voting_id, data = ?interaction.data, "vote not found");
        return Err(InteractionError::InternalServerError);
    };

    let vote = vote.parse::<i32>().map_err(|err| {
        tracing::error!(%voting_id, error = ?err, data = ?interaction.data, "parsing vote failed");
        InteractionError::InternalServerError
    })?;

    data
        .db
        .vote_voting_dialog(voting_id, &user_id.id.to_string(), vote, index)
        .await
    .map_err(|err| {
        tracing::error!(%voting_id, error = ?err, data = ?interaction.data, "updating vote in db failed");
        InteractionError::InternalServerError
    })?;

    ack_response()
}

async fn handle_vote_channel(
    data: &Arc<AppState>,
    interaction: &Interaction,
    voting_id: &str,
) -> InteractionResult {
    let voting = data.db.get_voting(voting_id).await.map_err(|err| {
        tracing::error!(%voting_id, error = ?err, data = ?interaction.data, "db get voting failed");
        InteractionError::InternalServerError
    })?;

    // this can happen with lingering dialogs while completing or deleting voting
    if voting.is_deleted || voting.is_completed {
        return ack_response();
    }

    let Some(ref member) = interaction.member else {
        tracing::error!(%voting_id, data = ?interaction.data, "member not found");
        return Err(InteractionError::InternalServerError);
    };

    let Some(ref user) = member.user else {
        tracing::error!(%voting_id, data = ?interaction.data, "user id not found");
        return Err(InteractionError::InternalServerError);
    };

    match data
        .db
        .save_voting_dialog(
            voting_id.to_string(),
            user.id.to_string(),
            Vec::new(),
            "".to_string(),
            "".to_string(),
            false,
        )
        .await
    {
        Ok(_) => (),
        Err(db::DbError::AlreadyExists) => {
            return Ok((StatusCode::OK, ephemeral_response("You already have voting dialog open or it is being sent to you. If that is not the case, please contact support.")));
        }
        Err(err) => {
            tracing::error!(%voting_id, error = ?err, data = ?interaction.data, "saving voting dialog into db failed");
            return Err(InteractionError::InternalServerError);
        }
    }

    let ballot: Vec<i32> = vec![0; voting.choices.len()];
    let (title, components, custom_ids) =
        create_vote_components(voting_id, voting, 1, ballot.clone());

    data.db.bulk_save_custom_ids(custom_ids).await.map_err(|err| {
        tracing::error!(%voting_id, error = ?err, data = ?interaction.data, "bulk saving custom ids into db failed");
        InteractionError::InternalServerError
    })?;

    let dm_channel = data.discord_client.create_private_channel(user.id).await.map_err(|err| {
        tracing::error!(%voting_id, error = ?err, data = ?interaction.data, "creating dm channel failed");
        InteractionError::InternalServerError
    })?;

    let dm_channel =  dm_channel
        .model()
        .await
        .map_err(|err| {
            tracing::error!(%voting_id, error = ?err, data = ?interaction.data, "getting dm channel model failed");
            InteractionError::InternalServerError
        })?;

    let message = create_message(&data.discord_client, dm_channel.id, &title, &components).await?;

    data
        .db
        .save_voting_dialog(
            voting_id.to_string(),
            user.id.to_string(),
            ballot.clone(),
            message.id.to_string(),
            dm_channel.id.to_string(),
            true,
        )
        .await
        .map_err(|err| {
            tracing::error!(%voting_id, error = ?err, data = ?interaction.data, "saving voting dialog into db failed");
            InteractionError::InternalServerError
        })?;

    let response = Json(InteractionResponse {
        kind: InteractionResponseType::ChannelMessageWithSource,
        data: Some(InteractionResponseData {
            content: Some("You will receive dm with voting dialog".to_string()),
            flags: Some(MessageFlags::EPHEMERAL),
            ..Default::default()
        }),
    });

    Ok((StatusCode::OK, response))
}

fn create_vote_components(
    voting_id: &str,
    voting: Voting,
    page: usize,
    ballot: Vec<i32>,
) -> (Vec<Embed>, Vec<Component>, Vec<(String, CustomID)>) {
    let page_size = 4;
    let total_pages = (voting.choices.len() + page_size - 1) / page_size;
    let start = (page - 1) * page_size;
    let end = usize::min(start + page_size, voting.choices.len());

    let paginated_choices = voting.choices[start..end]
        .iter()
        .enumerate()
        .map(|(i, choice)| format!("**{}**: {}", start + i + 1, choice))
        .collect::<Vec<_>>()
        .join("\n");

    let embed_title = if voting.choices.len() > page_size {
        format!("Voting Choices - Page {} of {}", page, total_pages)
    } else {
        "Voting Choices".to_string()
    };

    let title = EmbedBuilder::new()
        .title(embed_title)
        .description(paginated_choices)
        .build();

    let options: Vec<SelectMenuOption> = (1..=voting.choices.len())
        .map(|i| SelectMenuOption {
            default: false,
            description: None,
            emoji: None,
            label: i.to_string(),
            value: i.to_string(),
        })
        .collect();

    let mut custom_ids: Vec<(String, CustomID)> = Vec::new();

    let mut components: Vec<Component> = voting.choices[start..end]
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let placeholder = match ballot[i + start] {
                0 => "Select".to_string(),
                _ => ballot[i + start].to_string(),
            };

            let custom_uuid = util::generate_random_custom_uuid();
            let custom_id = CustomID {
                action: Action::VoteSelect,
                voting_id: voting_id.to_string(),
                user_id: None,
                page: None,
                index: Some(i + start),
            };

            custom_ids.push((custom_uuid.clone(), custom_id));

            Component::ActionRow(ActionRow {
                components: Vec::from([Component::SelectMenu(
                    twilight_model::channel::message::component::SelectMenu {
                        custom_id: custom_uuid,
                        disabled: false,
                        max_values: Some(1),
                        min_values: Some(1),
                        options: options.clone(),
                        placeholder: Some(placeholder),
                    },
                )]),
            })
        })
        .collect();

    let mut btns = Vec::new();

    if page > 1 {
        let custom_uuid = util::generate_random_custom_uuid();
        custom_ids.push((
            custom_uuid.clone(),
            CustomID {
                action: Action::VotePrevious,
                voting_id: voting_id.to_string(),
                user_id: None,
                page: Some(page - 1),
                index: None,
            },
        ));

        btns.push(Component::Button(Button {
            custom_id: Some(custom_uuid),
            disabled: false,
            emoji: None,
            label: Some("Previous".to_string()),
            style: ButtonStyle::Secondary,
            url: None,
        }));
    }

    if total_pages > page {
        let custom_uuid = util::generate_random_custom_uuid();
        custom_ids.push((
            custom_uuid.clone(),
            CustomID {
                action: Action::VoteNext,
                voting_id: voting_id.to_string(),
                user_id: None,
                page: Some(page + 1),
                index: None,
            },
        ));

        btns.push(Component::Button(Button {
            custom_id: Some(custom_uuid),
            disabled: false,
            emoji: None,
            label: Some("Next".to_string()),
            style: ButtonStyle::Secondary,
            url: None,
        }))
    }

    if page == total_pages {
        let custom_uuid = util::generate_random_custom_uuid();
        custom_ids.push((
            custom_uuid.clone(),
            CustomID {
                action: Action::VoteFromDM,
                voting_id: voting_id.to_string(),
                user_id: None,
                page: None,
                index: None,
            },
        ));
        btns.push(Component::Button(Button {
            custom_id: Some(custom_uuid),
            disabled: false,
            emoji: None,
            label: Some("Vote".to_string()),
            style: ButtonStyle::Primary,
            url: None,
        }))
    }

    if !btns.is_empty() {
        components.push(Component::ActionRow(ActionRow { components: btns }));
    }

    (vec![title], components, custom_ids)
}

async fn handle_slash_voting(
    data: &Arc<AppState>,
    command: &CommandData,
    interaction: &Interaction,
) -> InteractionResult {
    let Some(member) = interaction.member.as_ref() else {
        return Ok((
            StatusCode::OK,
            ephemeral_response("Voting can only be started from a public channel."),
        ));
    };

    let Some(option) = &command.options.first() else {
        tracing::error!(data = ?interaction, "option not found");
        return Err(InteractionError::InternalServerError);
    };

    let CommandOptionValue::String(ref name) = &option.value else {
        tracing::error!(data = ?interaction, "name not found");
        return Err(InteractionError::InternalServerError);
    };

    let choices: Vec<String> = command
        .options
        .iter()
        .skip(1)
        .filter_map(|option| match &option.value {
            CommandOptionValue::String(choice) => Some(choice.clone()),
            _ => None,
        })
        .collect();

    if choices.len() < 2 {
        tracing::error!(data = ?interaction, "voting must have at least 2 choices");
        return Ok((
            StatusCode::OK,
            ephemeral_response("Voting must have at least 2 choices."),
        ));
    }

    let voting = data
        .dd_client
        .create_voting(choices.clone())
        .await
        .map_err(|err| {
            tracing::error!(data= ?interaction, error = ?err, "creating voting failed");
            InteractionError::InternalServerError
        })?;

    let Some(ref user) = member.user else {
        tracing::error!(data = ?interaction, "user id not found");
        return Err(InteractionError::InternalServerError);
    };

    let dm_channel = data
        .discord_client
        .create_private_channel(user.id)
        .await
        .map_err(|err| {
            tracing::error!(data = ?interaction, error = ?err, "creating dm channel failed");
            InteractionError::InternalServerError
        })?
        .model()
        .await
        .map_err(|err| {
            tracing::error!(data = ?interaction, error = ?err, "getting dm channel model failed");
            InteractionError::InternalServerError
        })?;

    let embeds = vec![EmbedBuilder::new()
        .title(format!("Voting Created: {}", name))
        .description("Your voting has been successfully created. The results will be published once the voting is completed.")
        .field(EmbedFieldBuilder::new("Choices", choices.join("\n")))
        .build()];

    let mut custom_ids = Vec::new();
    let custom_uuid = util::generate_random_custom_uuid();
    custom_ids.push((
        custom_uuid.clone(),
        CustomID {
            action: Action::Complete,
            voting_id: voting.id.clone(),
            user_id: None,
            page: None,
            index: None,
        },
    ));

    let complete_btn = Button {
        custom_id: Some(custom_uuid),
        disabled: false,
        emoji: None,
        label: Some("Complete Voting".to_string()),
        style: ButtonStyle::Primary,
        url: None,
    };

    let custom_uuid = util::generate_random_custom_uuid();
    custom_ids.push((
        custom_uuid.clone(),
        CustomID {
            action: Action::Delete,
            voting_id: voting.id.clone(),
            user_id: None,
            page: None,
            index: None,
        },
    ));
    let delete_btn = Button {
        custom_id: Some(custom_uuid),
        disabled: false,
        emoji: None,
        label: Some("Delete Voting".to_string()),
        style: ButtonStyle::Danger,
        url: None,
    };

    let components = vec![Component::ActionRow(ActionRow {
        components: Vec::from([
            Component::Button(complete_btn),
            Component::Button(delete_btn),
        ]),
    })];

    let creator_message_id =
        create_message(&data.discord_client, dm_channel.id, &embeds, &components)
            .await?
            .id
            .to_string();

    let embeds = vec![EmbedBuilder::new()
        .title(format!(
            "Created a voting with name:{}, id: {} and choices: {:?}",
            name, voting.id, voting.choices
        ))
        .description("Click vote button when you are ready to vote. The voting will be done in dm.")
        .field(EmbedFieldBuilder::new("Choices", choices.join("\n")))
        .build()];

    let custom_uuid = util::generate_random_custom_uuid();
    let custom_id = CustomID {
        action: Action::VoteFromChannel,
        voting_id: voting.id.clone(),
        user_id: None,
        page: None,
        index: None,
    };

    let vote_btn = Button {
        custom_id: Some(custom_uuid.clone()),
        disabled: false,
        emoji: None,
        label: Some("Vote".to_string()),
        style: ButtonStyle::Primary,
        url: None,
    };

    custom_ids.push((custom_uuid, custom_id));

    data.db.bulk_save_custom_ids(custom_ids).await .map_err(|err| {
        tracing::error!(data = ?interaction, error = ?err, "bulk saving custom ids into db failed");
        InteractionError::InternalServerError
    })?;

    let components = vec![Component::ActionRow(ActionRow {
        components: Vec::from([Component::Button(vote_btn)]),
    })];

    let Some(ref channel) = interaction.channel else {
        tracing::error!(data = ?interaction, "channel not found");
        return Err(InteractionError::InternalServerError);
    };

    let message = create_message(&data.discord_client, channel.id, &embeds, &components).await?;

    data.db
        .save_voting(Voting {
            id: voting.id.clone(),
            name: name.to_string(),
            choices: choices.clone(),
            is_completed: false,
            is_deleted: false,
            message_id: message.id.to_string(),
            channel_id: message.channel_id.to_string(),
            creator_message_id,
            creator_dm_channel_id: dm_channel.id.to_string(),
        })
        .await
        .map_err(|err| {
            tracing::error!(data = ?interaction, error = ?err, "saving voting into db failed");
            InteractionError::InternalServerError
        })?;

    ack_response()
}

fn handle_ping() -> InteractionResult {
    let pong = Json(InteractionResponse {
        kind: InteractionResponseType::ChannelMessageWithSource,
        data: Some(InteractionResponseData {
            content: Some("pong".to_string()),
            ..Default::default()
        }),
    });

    Ok((StatusCode::OK, pong))
}

fn ephemeral_response(message: &str) -> Json<InteractionResponse> {
    Json(InteractionResponse {
        kind: InteractionResponseType::ChannelMessageWithSource,
        data: Some(InteractionResponseData {
            content: Some(message.to_string()),
            flags: Some(MessageFlags::EPHEMERAL),
            ..Default::default()
        }),
    })
}

fn ack_response() -> InteractionResult {
    Ok((
        StatusCode::OK,
        Json(InteractionResponse {
            kind: InteractionResponseType::ChannelMessageWithSource,
            data: None,
        }),
    ))
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum InteractionError {
    Status(StatusCode),
    InternalServerError,
}

impl IntoResponse for InteractionError {
    fn into_response(self) -> Response {
        match self {
            InteractionError::Status(status) => (status, "").into_response(),
            InteractionError::InternalServerError => (
                StatusCode::OK,
                ephemeral_response("Ouch, something went wrong. Please try again later."),
            )
                .into_response(),
        }
    }
}

async fn update_message(
    discord_client: &twilight_http::Client,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    content: Option<&str>,
    embeds: Option<&[Embed]>,
    components: Option<&[Component]>,
) -> Result<(), InteractionError> {
    discord_client
        .update_message(channel_id, message_id)
        .content(content)
        .map_err(|err| {
            tracing::error!(error = ?err, "message content failed");
            InteractionError::InternalServerError
        })?
        .embeds(embeds)
        .map_err(|err| {
            tracing::error!(error = ?err, "embeds failed");
            InteractionError::InternalServerError
        })?
        .components(components)
        .map_err(|err| {
            tracing::error!(error = ?err, "components failed");
            InteractionError::InternalServerError
        })?
        .await
        .map_err(|err| {
            tracing::error!(error = ?err, "updating message failed");
            InteractionError::InternalServerError
        })?;

    Ok(())
}

async fn create_message(
    discord_client: &twilight_http::Client,
    channel_id: Id<ChannelMarker>,
    embeds: &[Embed],
    components: &[Component],
) -> Result<Message, InteractionError> {
    let message = discord_client
        .create_message(channel_id)
        .embeds(embeds)
        .map_err(|err| {
            tracing::error!(error = ?err, "embeds failed");
            InteractionError::InternalServerError
        })?
        .components(components)
        .map_err(|err| {
            tracing::error!(error = ?err, "components failed");
            InteractionError::InternalServerError
        })?
        .await
        .map_err(|err| {
            tracing::error!(error = ?err, "creating message failed");
            InteractionError::InternalServerError
        })?
        .model()
        .await
        .map_err(|err| {
            tracing::error!(error = ?err, "getting message model failed");
            InteractionError::InternalServerError
        })?;

    Ok(message)
}
