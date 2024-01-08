use std::sync::Arc;

use dd_discord::db::Db;
use rand::Rng;
use redb::Database;

pub struct DropDb {
    name: String,
}
impl Drop for DropDb {
    fn drop(&mut self) {
        // remove test database
        std::fs::remove_file(self.name.clone()).expect("failed to remove test database");
    }
}

pub fn create_test_db() -> (DropDb, Db) {
    let name = format!("test-{}.redb", rand::thread_rng().gen::<u32>());
    let db = Database::create(name.clone()).expect("failed to create database");
    (DropDb { name }, Db { db: Arc::new(db) })
}
