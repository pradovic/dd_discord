mod common;
use common::create_test_db;
use dd_discord::db::{Action, CustomID, DbError, Voting};
use dd_discord::util;
use hex::encode;
use rand::Rng;

#[tokio::test]
async fn save_voting() {
    let (_drop_db, db) = create_test_db();
    let votings = vec![
        Voting {
            id: "84ee17be18185a077db2".to_string(),
            name: "voting1".to_string(),
            choices: vec!["choice1".to_string(), "choice2".to_string()],
            is_completed: false,
            is_deleted: false,
            message_id: "message_id".to_string(),
            channel_id: "channel_id".to_string(),
            creator_message_id: "creator_message_id".to_string(),
            creator_dm_channel_id: "creator_dm_channel_id".to_string(),
        },
        Voting {
            id: "84ee17be18185a077db3".to_string(),
            name: "voting2".to_string(),
            choices: vec!["choice1".to_string(), "choice2".to_string()],
            is_completed: false,
            is_deleted: false,
            message_id: "message_id".to_string(),
            channel_id: "channel_id".to_string(),
            creator_message_id: "creator_message_id".to_string(),
            creator_dm_channel_id: "creator_dm_channel_id".to_string(),
        },
        Voting {
            id: "84ee17be18185a077db4".to_string(),
            name: "voting2".to_string(),
            choices: vec!["choice1".to_string(), "choice2".to_string()],
            is_completed: false,
            is_deleted: false,
            message_id: "message_id".to_string(),
            channel_id: "channel_id".to_string(),
            creator_message_id: "creator_message_id".to_string(),
            creator_dm_channel_id: "creator_dm_channel_id".to_string(),
        },
    ];

    for voting in votings.iter() {
        db.save_voting(voting.clone())
            .await
            .expect("failed to save voting");
    }

    for voting in votings.iter() {
        let v = db
            .get_voting(&voting.id)
            .await
            .expect("failed to get voting");
        assert_eq!(v, *voting);
    }
}

#[tokio::test]
async fn voting_not_found() {
    let (_drop_db, db) = create_test_db();
    let voting_id = "84ee17be18185a077db2";

    let err = db
        .get_voting(voting_id)
        .await
        .expect_err("voting should not exist");

    assert_eq!(err, DbError::NotFound);
}

#[tokio::test]
async fn voting_already_exists() {
    let (_drop_db, db) = create_test_db();
    let voting_id = "84ee17be18185a077db2";
    let voting = Voting {
        id: voting_id.to_string(),
        name: "voting1".to_string(),
        choices: vec!["choice1".to_string(), "choice2".to_string()],
        is_completed: false,
        is_deleted: false,
        message_id: "message_id".to_string(),
        channel_id: "channel_id".to_string(),
        creator_message_id: "creator_message_id".to_string(),
        creator_dm_channel_id: "creator_dm_channel_id".to_string(),
    };

    db.save_voting(voting.clone())
        .await
        .expect("failed to save voting");

    let err = db
        .save_voting(voting)
        .await
        .expect_err("voting should already exist");

    assert_eq!(err, DbError::AlreadyExists);
}

#[tokio::test]
async fn complete_voting() {
    let (_drop_db, db) = create_test_db();
    let voting_id = "84ee17be18185a077db2";
    let voting = Voting {
        id: voting_id.to_string(),
        name: "voting1".to_string(),
        choices: vec!["choice1".to_string(), "choice2".to_string()],
        is_completed: false,
        is_deleted: false,
        message_id: "message_id".to_string(),
        channel_id: "channel_id".to_string(),
        creator_message_id: "creator_message_id".to_string(),
        creator_dm_channel_id: "creator_dm_channel_id".to_string(),
    };

    db.save_voting(voting.clone())
        .await
        .expect("failed to save voting");

    let v = db
        .get_voting(voting_id)
        .await
        .expect("failed to get voting");

    assert_eq!(v.is_completed, false);

    db.complete_voting(voting_id)
        .await
        .expect("failed to complete voting");

    let v = db
        .get_voting(voting_id)
        .await
        .expect("failed to get voting");

    assert_eq!(v.is_completed, true);
}

#[tokio::test]
async fn complete_voting_errors() {
    let (_drop_db, db) = create_test_db();
    let voting_id = "84ee17be18185a077db2";

    let err = db
        .complete_voting(voting_id)
        .await
        .expect_err("voting should not exist");

    assert_eq!(err, DbError::NotFound);

    let voting = Voting {
        id: voting_id.to_string(),
        name: "voting1".to_string(),
        choices: vec!["choice1".to_string(), "choice2".to_string()],
        is_completed: false,
        is_deleted: true,
        message_id: "message_id".to_string(),
        channel_id: "channel_id".to_string(),
        creator_message_id: "creator_message_id".to_string(),
        creator_dm_channel_id: "creator_dm_channel_id".to_string(),
    };

    db.save_voting(voting.clone())
        .await
        .expect("failed to save voting");

    db.complete_voting(voting_id)
        .await
        .expect_err("voting should be deleted");
}

#[tokio::test]
async fn delete_voting() {
    let (_drop_db, db) = create_test_db();
    let voting_id = "84ee17be18185a077db2";
    let voting = Voting {
        id: voting_id.to_string(),
        name: "voting1".to_string(),
        choices: vec!["choice1".to_string(), "choice2".to_string()],
        is_completed: false,
        is_deleted: false,
        message_id: "message_id".to_string(),
        channel_id: "channel_id".to_string(),
        creator_message_id: "creator_message_id".to_string(),
        creator_dm_channel_id: "creator_dm_channel_id".to_string(),
    };

    db.save_voting(voting.clone())
        .await
        .expect("failed to save voting");

    let v = db
        .get_voting(voting_id)
        .await
        .expect("failed to get voting");

    assert_eq!(v.is_deleted, false);

    db.delete_voting(voting_id)
        .await
        .expect("failed to delete voting");

    let v = db
        .get_voting(voting_id)
        .await
        .expect("failed to get voting");

    assert_eq!(v.is_deleted, true);
}

#[tokio::test]
async fn test_update_vote() {
    let (_drop_db, db) = create_test_db();
    let voting_id = "voting-id";
    let user_id = "user-id";
    let ballot = vec![0, 0, 0];

    db.save_voting_dialog(
        voting_id.to_string(),
        user_id.to_string(),
        ballot.clone(),
        "message_id".to_string(),
        "channel-id".to_string(),
        false,
    )
    .await
    .expect("failed to save voting dialog");

    let mut dialog = db
        .get_voting_dialog(voting_id, user_id)
        .await
        .expect("failed to get voting dialog");

    assert_eq!(&dialog.ballot, &ballot);

    db.vote_voting_dialog(voting_id, user_id, 1, 0)
        .await
        .expect("failed to update vote");

    let updated_dialog = db
        .get_voting_dialog(voting_id, user_id)
        .await
        .expect("failed to get voting dialog");

    dialog.ballot = vec![1, 0, 0];

    assert_eq!(dialog, updated_dialog);
}

#[tokio::test]
async fn test_update_vote_index_out_of_range() {
    let (_drop_db, db) = create_test_db();
    let voting_id = "voting-id";
    let user_id = "user-id";
    let ballot = vec![0, 0, 0];

    db.save_voting_dialog(
        voting_id.to_string(),
        user_id.to_string(),
        ballot.clone(),
        "message_id".to_string(),
        "channel-id".to_string(),
        false,
    )
    .await
    .expect("failed to save voting dialog");

    let err = db
        .vote_voting_dialog(voting_id, user_id, 1, 3)
        .await
        .expect_err("should not be able to update vote");

    assert_eq!(err, DbError::IndexOutOfRange);
}

#[tokio::test]
async fn test_update_vote_voting_dialog_not_found() {
    let (_drop_db, db) = create_test_db();
    let voting_id = "voting-id";
    let user_id = "user-id";

    let err = db
        .vote_voting_dialog(voting_id, user_id, 1, 0)
        .await
        .expect_err("should not be able to update vote");

    assert_eq!(err, DbError::NotFound);

    db.save_voting_dialog(
        voting_id.to_string(),
        user_id.to_string(),
        vec![0, 0, 0],
        "message-id".to_string(),
        "channel-id".to_string(),
        false,
    )
    .await
    .expect("failed to save voting dialog");

    let err = db
        .vote_voting_dialog(voting_id, "", 1, 0)
        .await
        .expect_err("should not be able to update vote");

    assert_eq!(err, DbError::NotFound);
}

#[tokio::test]
async fn test_save_voting_dialog() {
    let (_drop_db, db) = create_test_db();
    let voting_id = "voting-id";
    let user_id = "user-id";
    let ballot = vec![0, 0, 0];

    db.save_voting_dialog(
        voting_id.to_string(),
        user_id.to_string(),
        ballot.clone(),
        "message_id".to_string(),
        "channel-id".to_string(),
        true,
    )
    .await
    .expect("failed to save voting dialog");

    let dialog = db
        .get_voting_dialog(voting_id, user_id)
        .await
        .expect("failed to get voting dialog");

    assert_eq!(dialog.voting_id, voting_id);
    assert_eq!(dialog.user_id, user_id);
    assert_eq!(dialog.ballot, ballot);
}

#[tokio::test]
async fn test_get_voting_dialog_not_found() {
    let (_drop_db, db) = create_test_db();
    let voting_id = "voting-id";
    let user_id = "user-id";

    let err = db
        .get_voting_dialog(voting_id, user_id)
        .await
        .expect_err("voting dialog should not exist");

    assert_eq!(err, DbError::NotFound);
}

#[tokio::test]
async fn test_delete_voting_dialog() {
    let (_drop_db, db) = create_test_db();
    let voting_id = "voting-id";
    let user_id = "user-id";
    let ballot = vec![0, 0, 0];

    db.delete_voting_dialog(voting_id, user_id)
        .await
        .expect("failed to delete voting dialog");

    db.save_voting_dialog(
        voting_id.to_string(),
        user_id.to_string(),
        ballot.clone(),
        "message_id".to_string(),
        "channel-id".to_string(),
        false,
    )
    .await
    .expect("failed to save voting dialog");

    let dialog = db
        .get_voting_dialog(voting_id, user_id)
        .await
        .expect("failed to get voting dialog");

    assert_eq!(dialog.voting_id, voting_id);
    assert_eq!(dialog.user_id, user_id);
    assert_eq!(dialog.ballot, ballot);

    db.delete_voting_dialog(voting_id, user_id)
        .await
        .expect("failed to delete voting dialog");

    let err = db
        .get_voting_dialog(voting_id, user_id)
        .await
        .expect_err("voting dialog should not exist");

    assert_eq!(err, DbError::NotFound);
}

#[tokio::test]
async fn test_get_voting_dialogs() {
    let (_drop_db, db) = create_test_db();
    let voting_id1 = "84ee17be18185a077db2";
    let voting_id2 = "84ee17be18185a077db3";
    let ballot = vec![0, 0, 0];

    for _ in 0..100 {
        let user_id = generate_random_hex_string(20);
        db.save_voting_dialog(
            voting_id1.to_string(),
            user_id.to_string(),
            ballot.clone(),
            "message_id".to_string(),
            "channel_id".to_string(),
            false,
        )
        .await
        .expect("failed to save voting dialog");
    }

    for _ in 0..10 {
        let user_id = generate_random_hex_string(20);
        db.save_voting_dialog(
            voting_id2.to_string(),
            user_id.to_string(),
            ballot.clone(),
            "message_id".to_string(),
            "channel_id".to_string(),
            false,
        )
        .await
        .expect("failed to save voting dialog");
    }

    let dialogs = db
        .get_voting_dialogs(voting_id1)
        .await
        .expect("failed to get voting dialogs");

    assert_eq!(dialogs.len(), 100);

    let dialogs = db
        .get_voting_dialogs(voting_id2)
        .await
        .expect("failed to get voting dialogs");

    assert_eq!(dialogs.len(), 10);
}

#[tokio::test]
async fn test_custom_id() {
    let (_drop_db, db) = create_test_db();
    let voting_id = "84ee17be18185a077db2";
    let user_id = "user_id";
    let custom_id = "custom_id";

    let err = db
        .get_custom_id(custom_id)
        .await
        .expect_err("custom id should not exist");

    assert_eq!(err, DbError::NotFound);

    let custom_uuid = util::generate_random_custom_uuid();
    let custom_id = CustomID {
        action: Action::VoteFromChannel,
        voting_id: voting_id.to_string(),
        user_id: Some(user_id.to_string()),
        page: None,
        index: None,
    };

    db.bulk_save_custom_ids(vec![(custom_uuid.clone(), custom_id)])
        .await
        .expect("failed to save custom id");

    let custom_id = db
        .get_custom_id(&custom_uuid)
        .await
        .expect("failed to get custom id");

    assert_eq!(custom_id.voting_id, voting_id);
    assert_eq!(custom_id.user_id.unwrap(), user_id);

    let custom_ids = db
        .get_custom_ids(voting_id)
        .await
        .expect("failed to get custom ids");

    assert_eq!(custom_ids.len(), 1);

    let custom_uuid2 = util::generate_random_custom_uuid();
    let custom_uuid3 = util::generate_random_custom_uuid();
    let custom_id2 = CustomID {
        action: Action::VoteFromChannel,
        voting_id: voting_id.to_string(),
        user_id: Some(user_id.to_string()),
        page: None,
        index: None,
    };
    let custom_id3 = CustomID {
        action: Action::VoteFromChannel,
        voting_id: voting_id.to_string(),
        user_id: Some(user_id.to_string()),
        page: None,
        index: None,
    };

    // voting 2
    let voting_id2 = "84ee17be18185a077db3".to_string();
    let custom_uuid4 = util::generate_random_custom_uuid();
    let custom_id4 = CustomID {
        action: Action::VoteFromChannel,
        voting_id: voting_id2.clone(),
        user_id: Some(user_id.to_string()),
        page: None,
        index: None,
    };

    db.bulk_save_custom_ids(vec![
        (custom_uuid2, custom_id2),
        (custom_uuid3, custom_id3),
        (custom_uuid4, custom_id4),
    ])
    .await
    .expect("failed to save custom id");

    let custom_ids = db
        .get_custom_ids(voting_id)
        .await
        .expect("failed to get custom ids");

    assert_eq!(custom_ids.len(), 3);

    let custom_ids = db
        .get_custom_ids(&voting_id2)
        .await
        .expect("failed to get custom ids");

    assert_eq!(custom_ids.len(), 1);

    db.delete_custom_ids(voting_id)
        .await
        .expect("failed to delete custom ids");

    let custom_ids = db
        .get_custom_ids(voting_id)
        .await
        .expect("failed to get custom ids");

    assert_eq!(custom_ids.len(), 0);

    let custom_ids = db
        .get_custom_ids(&voting_id2)
        .await
        .expect("failed to get custom ids");

    assert_eq!(custom_ids.len(), 1);
}

#[tokio::test]
async fn test_delete_custom_ids() {
    let (_drop_db, db) = create_test_db();

    let voting_id1 = "voting-id1";
    let user_id1 = "user-id1";
    let voting_id2 = "voting-id2";
    let user_id2 = "user-id2";

    for _ in 0..10 {
        let custom_uuid = util::generate_random_custom_uuid();
        let custom_id = CustomID {
            action: Action::VoteFromChannel,
            voting_id: voting_id1.to_string(),
            user_id: Some(user_id1.to_string()),
            page: None,
            index: None,
        };

        db.bulk_save_custom_ids(vec![(custom_uuid, custom_id)])
            .await
            .expect("failed to save custom id");
    }

    for _ in 0..5 {
        let custom_uuid = util::generate_random_custom_uuid();
        let custom_id = CustomID {
            action: Action::VoteFromChannel,
            voting_id: voting_id2.to_string(),
            user_id: Some(user_id2.to_string()),
            page: None,
            index: None,
        };

        db.bulk_save_custom_ids(vec![(custom_uuid, custom_id)])
            .await
            .expect("failed to save custom id");
    }

    db.delete_custom_ids(voting_id1)
        .await
        .expect("failed to delete custom ids");

    let custom_ids = db
        .get_custom_ids(voting_id1)
        .await
        .expect("failed to get custom ids");

    assert_eq!(custom_ids.len(), 0);

    db.delete_custom_ids(voting_id2)
        .await
        .expect("failed to delete custom ids");

    let custom_ids = db
        .get_custom_ids(voting_id2)
        .await
        .expect("failed to get custom ids");

    assert_eq!(custom_ids.len(), 0);
}

fn generate_random_hex_string(length: usize) -> String {
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..length / 2).map(|_| rng.gen()).collect();
    encode(bytes)
}
