use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use log::{error, info, warn};
use serde::Deserialize;
use std::net::SocketAddr;
use std::sync::mpsc;

// Diese Struktur wird über den Channel gesendet und als JSON empfangen/gesendet.
// Sie muss `Clone` sein, für den State in Axum und das Senden über den Channel.
// Und `Send` + `'static` damit sie sicher über Threads gesendet werden kann.
#[derive(Deserialize, Debug, Clone)]
pub struct CommandPayload {
    pub command: String,
    pub value: serde_json::Value,
}

// AppState für den Axum-Server, hält den Sender des MPSC-Kanals.
// Muss Clone implementieren, damit Axum es für jeden Request klonen kann.
#[derive(Clone)]
struct AppState {
    command_tx: mpsc::Sender<CommandPayload>,
}

async fn status_handler() -> impl IntoResponse {
    info!("GET /status aufgerufen");
    (StatusCode::OK, "Server is running with Axum!")
}

async fn command_handler(
    State(state): State<AppState>,       // Extrahiert den AppState
    Json(payload): Json<CommandPayload>, // Extrahiert und deserialisiert die JSON-Payload
) -> impl IntoResponse {
    info!("POST /command empfangen: {:?}", payload);

    match state.command_tx.send(payload.clone()) {
        // Klonen, da payload auch für die Antwort verwendet wird
        Ok(_) => {
            info!("Befehl erfolgreich an den Hauptthread gesendet.");
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "command_received_and_forwarded",
                    "command": payload.command,
                    "value": payload.value
                })),
            )
        }
        Err(e) => {
            error!("Fehler beim Senden des Befehls an den Hauptthread: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "status": "error_forwarding_command",
                    "error": format!("Konnte Befehl nicht intern weiterleiten: {}", e)
                })),
            )
        }
    }
}

pub async fn run_axum_server(
    host: String,
    port: u16,
    command_tx: mpsc::Sender<CommandPayload>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Box<dyn Error> für generisches Fehlerhandling
    let app_state = AppState { command_tx };

    // Definiere die Routen
    let app = Router::new()
        .route("/status", get(status_handler))
        .route("/command", post(command_handler))
        .with_state(app_state); // Den State für alle Handler verfügbar machen

    let addr_str = format!("{}:{}", host, port);
    let addr: SocketAddr = addr_str.parse()?;

    info!("Axum Webserver startet auf {}", addr);

    // Server mit tokio::net::TcpListener starten
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?; // .into_make_service() ist oft nötig

    Ok(())
}

// Funktion, die in einem separaten Thread aufgerufen wird, um den Server zu starten
// und eine Tokio-Runtime dafür zu erstellen.
pub fn start_axum_server_in_thread(
    host: String,
    port: u16,
    command_tx: mpsc::Sender<CommandPayload>,
) {
    info!("Erstelle neuen Thread für Axum Webserver...");

    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            error!(
                "Konnte Tokio Runtime für Server-Thread nicht erstellen: {}",
                e
            );
            return;
        }
    };

    rt.block_on(async {
        if let Err(e) = run_axum_server(host, port, command_tx).await {
            error!("Axum Webserver-Fehler: {}", e);
        }
    });
    warn!("Axum Webserver-Thread wurde beendet.");
}
