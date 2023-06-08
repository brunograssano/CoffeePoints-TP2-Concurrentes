use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use lib::local_connection_messages::CoffeeMakerRequest;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServerMessage {
    pub message_type: ServerMessageType,
    pub sender_id: usize,
    pub passed_by: HashSet<usize>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ServerMessageType {
    NewConnection(Diff),
    CloseConnection,
    Token(TokenData),
    LostConnection(ServerId),
}

type ServerId = usize;
pub type TokenData = HashMap<usize, Vec<CoffeeMakerRequest>>;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Diff {
    pub last_update: Duration,
    pub changes: Vec<UpdatedAccount>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UpdatedAccount {
    pub id: usize,
    pub amount: usize,
    pub last_updated_on: Duration,
}

pub fn create_new_connection_message(sender_id: usize) -> ServerMessage {
    let diff = Diff {
        last_update: Duration::from_millis(1000), // TODO obtener last update o comienzo de fecha 1970
        changes: Vec::new(),
    };
    create_server_message(sender_id, ServerMessageType::NewConnection(diff))
}

pub fn create_token_message(sender_id: usize) -> ServerMessage {
    create_server_message(sender_id, ServerMessageType::Token(HashMap::new()))
}

pub fn create_lost_connection_message(sender_id: usize, to_id: usize) -> ServerMessage {
    create_server_message(sender_id, ServerMessageType::LostConnection(to_id))
}

fn create_server_message(sender_id: usize, message_type: ServerMessageType) -> ServerMessage {
    ServerMessage {
        message_type,
        sender_id,
        passed_by: HashSet::new(),
    }
}
