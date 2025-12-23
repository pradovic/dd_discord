use ed25519_dalek::{Signature, VerifyingKey};
use http::HeaderMap;
use reqwest::Method;
use twilight_model::application::command::CommandType;
use twilight_util::builder::command::{CommandBuilder, StringBuilder};
use uuid::Uuid;

// Register voting command to the bot
// This will overwrite the existing command if changed
// Panics if the request fails, which is fine because the bot should not work without the command
pub async fn register_voting_command(token: &str, api_url: &str, max_choices: usize) {
    let mut cmd = CommandBuilder::new("voting", "Create a voting", CommandType::ChatInput)
        .option(StringBuilder::new("name", "The reason of the voting").required(true))
        .option(StringBuilder::new("choice1", "The first choice").required(true));

    for i in 2..=max_choices {
        cmd = cmd.option(
            StringBuilder::new(format!("choice{i}"), format!("The {i}th choice"))
                .required(false),
        );
    }

    let client = reqwest::Client::new();
    let resp = client
        .request(Method::POST, api_url)
        .header("Authorization", format!("Bot {token}"))
        .json(&cmd.build())
        .send()
        .await
        .unwrap();

    tracing::info!("register voting comand: {}", resp.status());
}

// verify the signature of a request
// Return simple string error because the usage is simple, we just want to log the error
pub fn verify_signature(headers: &HeaderMap, body: &str, public_key: &str) -> Result<(), String> {
    let Some(signature) = headers.get("X-Signature-Ed25519") else {
        return Err("missing signature header".to_owned());
    };

    let Some(timestamp) = headers.get("X-Signature-Timestamp") else {
        return Err("missing timestamp header".to_owned());
    };

    let signature = hex::decode(signature.as_bytes()).map_err(|err| err.to_string())?;
    let signature = Signature::from_slice(&signature).map_err(|err| err.to_string())?;

    let mut signed_buf = timestamp.as_bytes().to_vec();
    signed_buf.extend_from_slice(body.as_bytes());

    let mut public_key_bytes: [u8; 32] = [0; 32];
    hex::decode_to_slice(public_key, &mut public_key_bytes as &mut [u8])
        .map_err(|err| err.to_string())?;
    let verifying_key =
        VerifyingKey::from_bytes(&public_key_bytes).map_err(|err| err.to_string())?;

    verifying_key
        .verify_strict(&signed_buf, &signature)
        .map_err(|err| err.to_string())?;
    Ok(())
}

#[must_use]
pub fn generate_random_custom_uuid() -> String {
    Uuid::new_v4().to_string()
}
