use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Status of a room
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoomStatus {
    Open,
    Full,
    Closed,
}

/// Information about a registered room
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomInfo {
    pub room_id: String,
    pub name: String,
    pub max_players: u8,
    pub current_players: u8,
    pub map_name: Option<String>,
    pub status: RoomStatus,
}

impl RoomInfo {
    pub fn new(room_id: String, name: String, max_players: u8, map_name: Option<String>) -> Self {
        Self {
            room_id,
            name,
            max_players,
            current_players: 0,
            map_name,
            status: RoomStatus::Open,
        }
    }

    /// Check if room has capacity for more players
    pub fn has_capacity(&self) -> bool {
        self.current_players < self.max_players && self.status == RoomStatus::Open
    }

    /// Increment player count and update status
    pub fn add_player(&mut self) {
        self.current_players += 1;
        if self.current_players >= self.max_players {
            self.status = RoomStatus::Full;
        }
    }

    /// Decrement player count and update status
    pub fn remove_player(&mut self) {
        if self.current_players > 0 {
            self.current_players -= 1;
        }
        if self.status == RoomStatus::Full && self.current_players < self.max_players {
            self.status = RoomStatus::Open;
        }
    }
}

/// Request body for creating a room
#[derive(Debug, Deserialize)]
pub struct CreateRoomRequest {
    pub room_id: String,
    pub name: String,
    pub max_players: u8,
    pub map_name: Option<String>,
}

/// Response for room creation
#[derive(Debug, Serialize)]
pub struct CreateRoomResponse {
    pub token: String,
}

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    /// Registered rooms: room_id -> RoomInfo
    pub rooms: Arc<RwLock<HashMap<String, RoomInfo>>>,
    /// Server tokens: token -> room_id
    pub tokens: Arc<RwLock<HashMap<String, String>>>,
    /// Active connections per room: room_id -> count
    pub connections: Arc<RwLock<HashMap<String, u8>>>,
    /// Matchbox server URL
    pub matchbox_url: String,
}

impl AppState {
    pub fn new(matchbox_url: String) -> Self {
        Self {
            rooms: Arc::new(RwLock::new(HashMap::new())),
            tokens: Arc::new(RwLock::new(HashMap::new())),
            connections: Arc::new(RwLock::new(HashMap::new())),
            matchbox_url,
        }
    }

    /// Register a new room and generate a token for the game server
    pub async fn register_room(&self, request: CreateRoomRequest) -> Result<String, String> {
        let mut rooms = self.rooms.write().await;

        if rooms.contains_key(&request.room_id) {
            return Err(format!("Room '{}' already exists", request.room_id));
        }

        let room = RoomInfo::new(
            request.room_id.clone(),
            request.name,
            request.max_players,
            request.map_name,
        );
        rooms.insert(request.room_id.clone(), room);

        // Generate token for game server
        let token = uuid::Uuid::new_v4().to_string();
        let mut tokens = self.tokens.write().await;
        tokens.insert(token.clone(), request.room_id);

        Ok(token)
    }

    /// Validate token and return the associated room_id
    pub async fn validate_token(&self, token: &str) -> Option<String> {
        let tokens = self.tokens.read().await;
        tokens.get(token).cloned()
    }

    /// Get a list of open rooms
    pub async fn list_rooms(&self) -> Vec<RoomInfo> {
        let rooms = self.rooms.read().await;
        rooms
            .values()
            .filter(|r| r.status == RoomStatus::Open || r.status == RoomStatus::Full)
            .cloned()
            .collect()
    }

    /// Get a specific room by ID
    pub async fn get_room(&self, room_id: &str) -> Option<RoomInfo> {
        let rooms = self.rooms.read().await;
        rooms.get(room_id).cloned()
    }

    /// Check if a room exists and has capacity
    pub async fn can_join_room(&self, room_id: &str) -> Result<(), String> {
        let rooms = self.rooms.read().await;
        match rooms.get(room_id) {
            Some(room) => {
                if room.has_capacity() {
                    Ok(())
                } else {
                    Err(format!("Room '{}' is full", room_id))
                }
            }
            None => Err(format!("Room '{}' not found", room_id)),
        }
    }

    /// Increment connection count for a room
    pub async fn add_connection(&self, room_id: &str) {
        let mut connections = self.connections.write().await;
        *connections.entry(room_id.to_string()).or_insert(0) += 1;

        // Update room player count
        let mut rooms = self.rooms.write().await;
        if let Some(room) = rooms.get_mut(room_id) {
            room.add_player();
        }
    }

    /// Decrement connection count for a room
    pub async fn remove_connection(&self, room_id: &str) {
        let mut connections = self.connections.write().await;
        if let Some(count) = connections.get_mut(room_id) {
            if *count > 0 {
                *count -= 1;
            }
        }

        // Update room player count
        let mut rooms = self.rooms.write().await;
        if let Some(room) = rooms.get_mut(room_id) {
            room.remove_player();
        }
    }

    /// Delete a room (requires valid token)
    pub async fn delete_room(&self, room_id: &str, token: &str) -> Result<(), String> {
        // Verify token matches room
        let tokens = self.tokens.read().await;
        let valid = tokens.get(token).map(|id| id == room_id).unwrap_or(false);
        drop(tokens);

        if !valid {
            return Err("Invalid token".to_string());
        }

        self.delete_room_by_host(room_id).await;
        Ok(())
    }

    /// Delete a room when host disconnects (internal use)
    pub async fn delete_room_by_host(&self, room_id: &str) {
        // Remove room
        let mut rooms = self.rooms.write().await;
        if rooms.remove(room_id).is_some() {
            tracing::info!(room_id = %room_id, "Room deleted (host disconnected)");
        }
        drop(rooms);

        // Find and remove associated token
        let mut tokens = self.tokens.write().await;
        tokens.retain(|_, rid| rid != room_id);
        drop(tokens);

        // Remove connections tracking
        let mut connections = self.connections.write().await;
        connections.remove(room_id);
    }
}
