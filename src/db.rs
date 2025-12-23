use redb::{
    CommitError, Database, ReadableTable as _, StorageError, TableDefinition, TransactionError,
};
use serde::{Deserialize, Serialize};
use std::{fmt::Display, sync::Arc};
use tokio::task::JoinError;

// <votingID, votingJson>
const VOTING_TABLE: TableDefinition<&str, &str> = TableDefinition::new("voting");
// <votingID-userID, votingDialogJson>
const VOTING_DIALOG_TABLE: TableDefinition<&str, &str> = TableDefinition::new("voting_dialog");
// <customUUID, customIDJson>
const CUSTOM_ID_TABLE: TableDefinition<&str, &str> = TableDefinition::new("custom_id");
// <votingID-customUUID, customUUID>
const VOTING_CUSTOMID_INDEX_TABLE: TableDefinition<&str, &str> =
    TableDefinition::new("voting_customid_index");
const ENCODE_DELIMITER: &str = "-";

pub struct Db {
    pub db: Arc<Database>,
}

#[must_use]
pub fn new() -> Db {
    let db = Database::create("voting.redb").expect("failed to create database");
    Db { db: Arc::new(db) }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct Voting {
    pub id: String,
    pub name: String,
    pub choices: Vec<String>,
    pub is_completed: bool,
    pub is_deleted: bool,
    pub message_id: String,
    pub channel_id: String,
    pub creator_message_id: String,
    pub creator_dm_channel_id: String,
}

impl TryFrom<&str> for Voting {
    type Error = DbError;

    fn try_from(voting: &str) -> Result<Self, Self::Error> {
        serde_json::from_str(voting).map_err(|e| DbError::Other(e.to_string()))
    }
}

impl From<&Voting> for String {
    fn from(voting: &Voting) -> Self {
        serde_json::to_string(&voting).expect("failed to serialize voting")
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoteDialog {
    pub voting_id: String,
    pub user_id: String,
    pub ballot: Vec<i32>,
    pub message_id: String,
    pub channel_id: String,
}

impl TryFrom<&str> for VoteDialog {
    type Error = DbError;

    fn try_from(dialog: &str) -> Result<Self, Self::Error> {
        serde_json::from_str(dialog).map_err(|e| DbError::Other(e.to_string()))
    }
}

impl From<&VoteDialog> for String {
    fn from(dialog: &VoteDialog) -> Self {
        serde_json::to_string(&dialog).expect("failed to serialize voting dialog")
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq, Clone)]
pub struct CustomID {
    pub action: Action,
    pub voting_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
}

impl Display for CustomID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "{}", serde_json::to_string(self).unwrap())
    }
}

impl TryFrom<&str> for CustomID {
    type Error = DbError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        serde_json::from_str(s).map_err(|e| DbError::Other(e.to_string()))
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq, Clone)]
pub enum Action {
    VoteFromChannel,
    VoteFromDM,
    VoteSelect,
    VoteNext,
    VotePrevious,
    Complete,
    Delete,
}

#[derive(Debug, PartialEq, Eq)]
pub enum DbError {
    NotFound,
    IndexOutOfRange,
    AlreadyExists,
    Other(String),
}

impl From<redb::TableError> for DbError {
    fn from(e: redb::TableError) -> Self {
        if let redb::TableError::TableDoesNotExist(_) = e {
            Self::NotFound
        } else {
            Self::Other(e.to_string())
        }
    }
}

impl From<redb::Error> for DbError {
    fn from(e: redb::Error) -> Self {
        Self::Other(e.to_string())
    }
}

impl From<TransactionError> for DbError {
    fn from(e: TransactionError) -> Self {
        Self::Other(e.to_string())
    }
}

impl From<CommitError> for DbError {
    fn from(e: CommitError) -> Self {
        Self::Other(e.to_string())
    }
}

impl From<StorageError> for DbError {
    fn from(e: StorageError) -> Self {
        Self::Other(e.to_string())
    }
}

impl From<JoinError> for DbError {
    fn from(e: JoinError) -> Self {
        Self::Other(e.to_string())
    }
}

impl Db {
    // Saves voting to the database.
    // Returns `AlreadyExists` if the voting with the same id already exists.
    pub async fn save_voting(&self, voting: Voting) -> Result<(), DbError> {
        let db = Arc::<Database>::clone(&self.db);

        tokio::task::spawn_blocking(move || {
            let write_txn = db.begin_write()?;
            {
                let mut table = write_txn.open_table(VOTING_TABLE)?;

                if table.get(voting.id.as_str())?.is_some() {
                    return Err(DbError::AlreadyExists);
                }
                table.insert(voting.id.clone().as_str(), String::from(&voting).as_str())?;
            };

            write_txn.commit()?;

            Ok(())
        })
        .await?
    }

    // Marks voting as completed.
    // Returns `NotFound` if the voting is not found, or if it was marked as deleted.
    pub async fn complete_voting(&self, id: &str) -> Result<Voting, DbError> {
        let db = Arc::<Database>::clone(&self.db);
        let id = id.to_owned();

        tokio::task::spawn_blocking(move || {
            let read_txn = db.begin_read()?;

            let table = read_txn.open_table(VOTING_TABLE)?;

            let res = table.get(id.as_str())?;

            match res {
                Some(v) => {
                    let mut voting = Voting::try_from(v.value())?;
                    if voting.is_deleted {
                        return Err(DbError::NotFound);
                    }

                    voting.is_completed = true;

                    let write_txn = db.begin_write()?;
                    {
                        let mut table = write_txn.open_table(VOTING_TABLE)?;
                        table.insert(id.as_str(), String::from(&voting).as_str())?;
                    };

                    write_txn.commit()?;
                    Ok(voting)
                }
                None => Err(DbError::NotFound),
            }
        })
        .await?
    }

    pub async fn delete_voting(&self, id: &str) -> Result<Voting, DbError> {
        let db = Arc::<Database>::clone(&self.db);
        let id = id.to_owned();

        tokio::task::spawn_blocking(move || {
            let read_txn = db.begin_read()?;

            let table = read_txn.open_table(VOTING_TABLE)?;

            let res = table.get(id.as_str())?;

            match res {
                Some(v) => {
                    let mut voting = Voting::try_from(v.value())?;
                    if voting.is_deleted {
                        return Err(DbError::NotFound);
                    }

                    voting.is_deleted = true;

                    let write_txn = db.begin_write()?;
                    {
                        let mut table = write_txn.open_table(VOTING_TABLE)?;
                        table.insert(id.as_str(), String::from(&voting).as_str())?;
                    };

                    write_txn.commit()?;
                    Ok(voting)
                }
                None => Err(DbError::NotFound),
            }
        })
        .await?
    }

    // Get voting for the provided id.
    // Voting marked as deleted or completed are returned successfully.
    // It is up to the caller to check the state of the voting
    pub async fn get_voting(&self, id: &str) -> Result<Voting, DbError> {
        let db = Arc::<Database>::clone(&self.db);
        let id = id.to_owned();

        tokio::task::spawn_blocking(move || {
            let read_txn = db.begin_read()?;

            let table = read_txn.open_table(VOTING_TABLE)?;

            let res = table.get(id.as_str())?;

            match res {
                Some(v) => Ok(Voting::try_from(v.value())?),
                None => Err(DbError::NotFound),
            }
        })
        .await
        .map_err(|e| DbError::Other(e.to_string()))?
    }

    /// Updates vote value in the ballot of the voting dialog.
    /// Index is the index of the choice in the ballot. It starts from 0.
    /// Returns `IndexOutOfRange` if the index is bigger than the ballot size.
    pub async fn vote_voting_dialog(
        &self,
        voting_id: &str,
        user_id: &str,
        vote: i32,
        index: usize,
    ) -> Result<(), DbError> {
        let id = encode_key(voting_id, user_id);
        let db = Arc::<Database>::clone(&self.db);

        tokio::task::spawn_blocking(move || {
            let read_txn = db.begin_read()?;

            let table = read_txn.open_table(VOTING_DIALOG_TABLE)?;
            let res = table.get(id.as_str())?;

            match res {
                Some(v) => {
                    let mut voting_dialog = VoteDialog::try_from(v.value())?;
                    if index >= voting_dialog.ballot.len() {
                        return Err(DbError::IndexOutOfRange);
                    }

                    voting_dialog.ballot[index] = vote;

                    let write_txn = db.begin_write()?;
                    {
                        let mut table = write_txn.open_table(VOTING_DIALOG_TABLE)?;
                        table.insert(id.as_str(), String::from(&voting_dialog).as_str())?;
                    };

                    write_txn.commit()?;
                    Ok(())
                }
                None => Err(DbError::NotFound),
            }
        })
        .await
        .map_err(|e| DbError::Other(e.to_string()))?
    }

    // Saves voting dialog to the database.
    // Returns `AlreadyExists` if the dialog with the same voting id and user id already exists.
    pub async fn save_voting_dialog(
        &self,
        voting_id: String,
        user_id: String,
        ballot: Vec<i32>,
        message_id: String,
        channel_id: String,
        overwrite: bool,
    ) -> Result<(), DbError> {
        let id = encode_key(&voting_id, &user_id);
        let dialog = VoteDialog {
            voting_id,
            user_id,
            ballot,
            message_id,
            channel_id,
        };

        let db = Arc::<Database>::clone(&self.db);

        tokio::task::spawn_blocking(move || {
            let write_txn = db.begin_write()?;
            {
                let mut table = write_txn.open_table(VOTING_DIALOG_TABLE)?;

                if !overwrite && table.get(id.as_str())?.is_some() {
                    return Err(DbError::AlreadyExists);
                }

                table.insert(id.as_str(), String::from(&dialog).as_str())?;
            };

            write_txn.commit()?;

            Ok(())
        })
        .await
        .map_err(|e| DbError::Other(e.to_string()))?
    }

    pub async fn get_voting_dialog(
        &self,
        voting_id: &str,
        user_id: &str,
    ) -> Result<VoteDialog, DbError> {
        let id = encode_key(voting_id, user_id);
        let db = Arc::<Database>::clone(&self.db);

        tokio::task::spawn_blocking(move || {
            let read_txn = db.begin_read()?;

            let table = read_txn.open_table(VOTING_DIALOG_TABLE)?;

            let res = table.get(id.as_str())?;

            match res {
                Some(v) => Ok(VoteDialog::try_from(v.value())?),
                None => Err(DbError::NotFound),
            }
        })
        .await
        .map_err(|e| DbError::Other(e.to_string()))?
    }

    pub async fn get_voting_dialogs(&self, voting_id: &str) -> Result<Vec<VoteDialog>, DbError> {
        let db = Arc::<Database>::clone(&self.db);
        let voting_id = voting_id.to_owned();

        tokio::task::spawn_blocking(move || {
            let read_txn = db.begin_read()?;

            let table = read_txn.open_table(VOTING_DIALOG_TABLE)?;

            let res = table.range(format!("{voting_id}{ENCODE_DELIMITER}").as_str()..)?;

            let mut dialogs = vec![];
            for v in res.flatten() {
                let dialog = VoteDialog::try_from(v.1.value())?;
                if dialog.voting_id == voting_id {
                    dialogs.push(dialog);
                }
            }

            Ok(dialogs)
        })
        .await
        .map_err(|e| DbError::Other(e.to_string()))?
    }

    pub async fn delete_voting_dialog(
        &self,
        voting_id: &str,
        user_id: &str,
    ) -> Result<(), DbError> {
        let id = encode_key(voting_id, user_id);
        let db = Arc::<Database>::clone(&self.db);

        tokio::task::spawn_blocking(move || {
            let write_txn = db.begin_write()?;
            {
                let mut table = write_txn.open_table(VOTING_DIALOG_TABLE)?;
                table.remove(id.as_str())?;
            };

            write_txn.commit()?;

            Ok(())
        })
        .await
        .map_err(|e| DbError::Other(e.to_string()))?
    }

    pub async fn bulk_save_custom_ids(
        &self,
        custom_ids: Vec<(String, CustomID)>,
    ) -> Result<(), DbError> {
        let db = Arc::<Database>::clone(&self.db);

        tokio::task::spawn_blocking(move || {
            let write_txn = db.begin_write()?;
            {
                let mut table = write_txn.open_table(CUSTOM_ID_TABLE)?;
                let mut index_table = write_txn.open_table(VOTING_CUSTOMID_INDEX_TABLE)?;

                for (custom_uuid, custom_id) in custom_ids {
                    table.insert(custom_uuid.as_str(), custom_id.to_string().as_str())?;
                    let index_key = encode_key(&custom_id.voting_id, &custom_uuid);
                    index_table.insert(index_key.as_str(), custom_uuid.as_str())?;
                }
            }

            write_txn.commit()?;

            Ok(())
        })
        .await
        .map_err(|e| DbError::Other(e.to_string()))?
    }

    pub async fn get_custom_id(&self, id: &str) -> Result<CustomID, DbError> {
        let db = Arc::<Database>::clone(&self.db);
        let id = id.to_owned();

        tokio::task::spawn_blocking(move || {
            let read_txn = db.begin_read()?;

            let table = read_txn.open_table(CUSTOM_ID_TABLE)?;

            let res = table.get(id.as_str())?;

            match res {
                Some(v) => Ok(CustomID::try_from(v.value())?),
                None => Err(DbError::NotFound),
            }
        })
        .await
        .map_err(|e| DbError::Other(e.to_string()))?
    }

    pub async fn get_custom_ids(&self, voting_id: &str) -> Result<Vec<CustomID>, DbError> {
        let db = Arc::<Database>::clone(&self.db);
        let voting_id = voting_id.to_owned();

        tokio::task::spawn_blocking(move || {
            let read_txn = db.begin_read()?;

            let table = read_txn.open_table(CUSTOM_ID_TABLE)?;

            let table_index = read_txn.open_table(VOTING_CUSTOMID_INDEX_TABLE)?;

            let index_prefix = format!("{voting_id}{ENCODE_DELIMITER}");

            let res = table_index.range(index_prefix.as_str()..)?;

            let mut custom_ids = vec![];
            for v in res.flatten() {
                let index = v.0.value();
                if !index.starts_with(index_prefix.as_str()) {
                    break;
                }

                let custom_uuid = v.1.value();

                let v = table.get(custom_uuid);
                if let Ok(Some(custom_id_v)) = v {
                    let custom_id = CustomID::try_from(custom_id_v.value())?;
                    custom_ids.push(custom_id);
                } else {
                    tracing::error!("failed to get custom id for index: {}", index);
                }
            }

            Ok(custom_ids)
        })
        .await
        .map_err(|e| DbError::Other(e.to_string()))?
    }

    pub async fn delete_custom_ids(&self, voting_id: &str) -> Result<(), DbError> {
        let db = Arc::<Database>::clone(&self.db);
        let voting_id = voting_id.to_owned();

        tokio::task::spawn_blocking(move || {
            let write_txn = db.begin_write()?;
            {
                let mut custom_id_table = write_txn.open_table(CUSTOM_ID_TABLE)?;

                let mut index_table = write_txn.open_table(VOTING_CUSTOMID_INDEX_TABLE)?;

                let index_prefix = format!("{voting_id}{ENCODE_DELIMITER}");

                let mut to_remove: Vec<(String, String)> = Vec::new();
                {
                    let res = index_table.range(index_prefix.as_str()..)?;

                    // (index, custom_uuid)
                    for v in res.flatten() {
                        let index = v.0.value();
                        if !index.starts_with(index_prefix.as_str()) {
                            break;
                        }

                        to_remove.push((index.to_owned(), v.1.value().to_owned()));
                    }
                }

                for (index, custom_uuid) in to_remove {
                    custom_id_table.remove(custom_uuid.as_str())?;
                    index_table.remove(index.as_str())?;
                }
            }

            write_txn.commit()?;

            Ok(())
        })
        .await
        .map_err(|e| DbError::Other(e.to_string()))?
    }
}

fn encode_key(voting_id: &str, user_id: &str) -> String {
    format!("{voting_id}{ENCODE_DELIMITER}{user_id}")
}
