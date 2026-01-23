use axum::{
    extract::{
        ws::{Message, WebSocket},
        Path, Query, State, WebSocketUpgrade,
    },
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio_tungstenite::{connect_async, tungstenite::Message as TungsteniteMessage};

use crate::state::AppState;

/// Query params for server connection
#[derive(Deserialize)]
pub struct ServerConnectQuery {
    token: String,
}

/// Handle WebSocket connection from game server
/// Endpoint: /connect?token=X
pub async fn handle_server_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(query): Query<ServerConnectQuery>,
) -> Response {
    // Validate token
    let room_id = match state.validate_token(&query.token).await {
        Some(id) => id,
        None => {
            tracing::warn!("Invalid server token attempted connection");
            // Return HTTP error instead of WebSocket close to avoid confusing matchbox_socket
            return axum::response::Response::builder()
                .status(axum::http::StatusCode::UNAUTHORIZED)
                .body(axum::body::Body::from("Invalid token"))
                .unwrap();
        }
    };

    tracing::info!(room_id = %room_id, "Game server connecting");

    let matchbox_url = format!("{}/{}", state.matchbox_url, room_id);

    ws.on_upgrade(move |socket| async move {
        if let Err(e) = proxy_websocket(socket, &matchbox_url, Some((state, room_id))).await {
            tracing::error!("Server WebSocket proxy error: {}", e);
        }
    })
}

/// Handle WebSocket connection from client
/// Endpoint: /{room_id}
pub async fn handle_client_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
) -> Response {
    // Validate room exists and has capacity
    if let Err(e) = state.can_join_room(&room_id).await {
        tracing::warn!(room_id = %room_id, error = %e, "Client connection rejected");
        // Return HTTP error instead of WebSocket close to avoid confusing matchbox_socket
        return axum::response::Response::builder()
            .status(axum::http::StatusCode::NOT_FOUND)
            .body(axum::body::Body::from(e))
            .unwrap();
    }

    tracing::info!(room_id = %room_id, "Client connecting");

    let matchbox_url = format!("{}/{}", state.matchbox_url, room_id);
    let room_id_clone = room_id.clone();

    ws.on_upgrade(move |socket| async move {
        // Track connection
        state.add_connection(&room_id_clone).await;

        let result = proxy_websocket(socket, &matchbox_url, None).await;

        // Untrack connection
        state.remove_connection(&room_id_clone).await;

        if let Err(e) = result {
            tracing::error!(room_id = %room_id_clone, "Client WebSocket proxy error: {}", e);
        }
    })
}

/// Proxy WebSocket messages bidirectionally between client and matchbox
async fn proxy_websocket(
    client_ws: WebSocket,
    matchbox_url: &str,
    server_tracking: Option<(AppState, String)>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Connect to matchbox server
    let (matchbox_ws, _response) = connect_async(matchbox_url).await.map_err(|e| {
        tracing::error!(url = %matchbox_url, "Failed to connect to matchbox: {}", e);
        e
    })?;

    tracing::debug!(url = %matchbox_url, "Connected to matchbox server");

    // If this is a server connection, track it
    if let Some((ref state, ref room_id)) = server_tracking {
        state.add_connection(room_id).await;
    }

    let (mut client_sink, mut client_stream) = client_ws.split();
    let (mut matchbox_sink, mut matchbox_stream) = matchbox_ws.split();

    // Forward client -> matchbox
    let client_to_matchbox = async {
        while let Some(msg) = client_stream.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if matchbox_sink
                        .send(TungsteniteMessage::Text(text))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(Message::Binary(data)) => {
                    if matchbox_sink
                        .send(TungsteniteMessage::Binary(data))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(Message::Ping(data)) => {
                    if matchbox_sink
                        .send(TungsteniteMessage::Ping(data))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(Message::Pong(data)) => {
                    if matchbox_sink
                        .send(TungsteniteMessage::Pong(data))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(Message::Close(_)) | Err(_) => break,
            }
        }
        let _ = matchbox_sink.close().await;
    };

    // Forward matchbox -> client
    let matchbox_to_client = async {
        while let Some(msg) = matchbox_stream.next().await {
            match msg {
                Ok(TungsteniteMessage::Text(text)) => {
                    if client_sink
                        .send(Message::Text(text.to_string()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(TungsteniteMessage::Binary(data)) => {
                    if client_sink
                        .send(Message::Binary(data.to_vec()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(TungsteniteMessage::Ping(data)) => {
                    if client_sink
                        .send(Message::Ping(data.to_vec()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(TungsteniteMessage::Pong(data)) => {
                    if client_sink
                        .send(Message::Pong(data.to_vec()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(TungsteniteMessage::Close(_)) | Err(_) => break,
                Ok(TungsteniteMessage::Frame(_)) => {}
            }
        }
        let _ = client_sink.close().await;
    };

    // Run both directions concurrently, stop when either ends
    tokio::select! {
        _ = client_to_matchbox => {
            tracing::debug!("Client connection closed");
        }
        _ = matchbox_to_client => {
            tracing::debug!("Matchbox connection closed");
        }
    }

    // If this was a server connection, untrack it
    if let Some((state, room_id)) = server_tracking {
        state.remove_connection(&room_id).await;
    }

    Ok(())
}
