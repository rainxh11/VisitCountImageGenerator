use ab_glyph::FontArc;
use axum::{
    Json, Router,
    extract::{Path as RoutePath, State},
    http::{HeaderMap, StatusCode, Uri, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use image::{ImageFormat, Rgba, RgbaImage};
use imageproc::drawing::{draw_text_mut, text_size};
use rusqlite::{Connection, params};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::Cursor,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

type AppResult<T> = Result<T, StatusCode>;

#[derive(Clone)]
pub struct AppState {
    db: Arc<Mutex<Connection>>,
    background: Arc<RgbaImage>,
    font: FontArc,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Visit {
    username: String,
    request: Value,
    id: i64,
    created_on: String,
}

impl AppState {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Box<dyn std::error::Error>> {
        if let Some(parent) = path.as_ref().parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let db = Connection::open(path)?;
        db.busy_timeout(Duration::from_secs(5))?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS visits (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                username TEXT NOT NULL,
                request TEXT NOT NULL,
                created_on TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS visits_username_id ON visits(username, id DESC);",
        )?;
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            background: Arc::new(
                image::load_from_memory(include_bytes!("../VisitCount.png"))?.to_rgba8(),
            ),
            font: FontArc::try_from_slice(include_bytes!("../Caskaydia.otf"))?,
        })
    }

    async fn blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce(&mut Connection, &Self) -> AppResult<T> + Send + 'static,
    ) -> AppResult<T> {
        let state = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut db = state
                .db
                .lock()
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            work(&mut db, &state)
        })
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    }

    fn image(&self, count: i64) -> AppResult<Vec<u8>> {
        let digits = count.to_string();
        let text: String = digits
            .chars()
            .enumerate()
            .flat_map(|(i, c)| {
                let mut part = String::new();
                if i > 0 && (digits.len() - i).is_multiple_of(3) {
                    part.push(',');
                }
                part.push(c);
                part.chars().collect::<Vec<_>>()
            })
            .collect();
        let mut image = (*self.background).clone();
        let scale = 48.0;
        let (width, _) = text_size(scale, &self.font, &text);
        // Retain the original image layout.
        let y = 10;
        draw_text_mut(
            &mut image,
            Rgba([0, 255, 255, 255]),
            1080 - width as i32,
            y,
            scale,
            &self.font,
            &text,
        );
        let zeros = "0".repeat(9usize.saturating_sub(text.len()).max(1));
        draw_text_mut(
            &mut image,
            Rgba([128, 128, 128, 255]),
            825,
            y,
            scale,
            &self.font,
            &zeros,
        );
        round_corners(&mut image, 20.0);
        let mut output = Cursor::new(Vec::new());
        image
            .write_to(&mut output, ImageFormat::Png)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(output.into_inner())
    }
}

fn round_corners(image: &mut RgbaImage, radius: f32) {
    let (width, height) = image.dimensions();
    for y in 0..height {
        for x in 0..width {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let cx = px.clamp(radius, width as f32 - radius);
            let cy = py.clamp(radius, height as f32 - radius);
            let distance = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
            let coverage = (radius + 0.5 - distance).clamp(0.0, 1.0);
            let pixel = image.get_pixel_mut(x, y);
            pixel[3] = (pixel[3] as f32 * coverage) as u8;
        }
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/visit-count/query", get(query_all))
        .route("/visit-count/query/{username}", get(query_user))
        .route("/visit-count/generate/{filename}", get(generate))
        .with_state(state)
        .layer(middleware::from_fn(no_cache))
}

async fn no_cache(request: axum::extract::Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        "max-age=0, no-cache, no-store, must-revalidate"
            .parse()
            .unwrap(),
    );
    response
}

fn visits(db: &Connection, username: Option<&str>) -> AppResult<Vec<Visit>> {
    let mut statement = db.prepare("SELECT username, request, id, created_on FROM visits WHERE (?1 IS NULL OR username = ?1) ORDER BY id DESC")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let rows = statement
        .query_map([username], |row| {
            let request: String = row.get(1)?;
            Ok((
                row.get::<_, String>(0)?,
                request,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    rows.map(|row| {
        let (username, request, id, created_on) =
            row.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(Visit {
            username,
            request: serde_json::from_str(&request)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            id,
            created_on,
        })
    })
    .collect()
}

async fn query_user(
    State(state): State<AppState>,
    RoutePath(username): RoutePath<String>,
) -> AppResult<Json<Vec<Visit>>> {
    state
        .blocking(move |db, _| visits(db, Some(&username)).map(Json))
        .await
}

async fn query_all(State(state): State<AppState>) -> AppResult<Json<Value>> {
    state
        .blocking(|db, _| {
            let mut grouped: BTreeMap<String, Vec<Visit>> = BTreeMap::new();
            for visit in visits(db, None)? {
                grouped
                    .entry(visit.username.clone())
                    .or_default()
                    .push(visit);
            }
            let stats: BTreeMap<_, _> = grouped
                .iter()
                .map(|(username, visits)| {
                    let visitors: Vec<_> = visits
                        .iter()
                        .filter_map(|visit| {
                            visit.request["Headers"]
                                .as_array()?
                                .iter()
                                .find(|value| {
                                    value.as_str().is_some_and(|s| {
                                        s.to_ascii_lowercase().contains("x-forwarded-for")
                                    })
                                })
                                .cloned()
                        })
                        .collect();
                    (
                        username,
                        json!({"count": visits.len(), "visitors": visitors}),
                    )
                })
                .collect();
            Ok(Json(json!({"stats": stats, "grouped": grouped})))
        })
        .await
}

async fn generate(
    State(state): State<AppState>,
    RoutePath(filename): RoutePath<String>,
    headers: HeaderMap,
    uri: Uri,
) -> AppResult<Response> {
    let username = filename
        .strip_suffix(".png")
        .filter(|s| !s.is_empty())
        .ok_or(StatusCode::NOT_FOUND)?
        .to_owned();
    let cookies: Vec<_> = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|h| h.to_str().ok())
        .flat_map(|s| s.split(';'))
        .filter_map(|s| s.trim().split_once('='))
        .map(|(key, value)| json!({"Key": key, "Value": value}))
        .collect();
    let request_headers: Vec<_> = headers
        .iter()
        .map(|(key, value)| format!("[{}, {}]", key, value.to_str().unwrap_or_default()))
        .collect();
    let query: std::collections::BTreeSet<_> =
        url::form_urlencoded::parse(uri.query().unwrap_or_default().as_bytes())
            .map(|(key, _)| key.into_owned())
            .collect();
    let request = json!({"Cookies": cookies, "Headers": request_headers, "Host": headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or_default(), "Query": query, "Path": uri.path()});
    state
        .blocking(move |db, state| {
            let transaction = db
                .transaction()
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            transaction
                .execute(
                    "INSERT INTO visits(username, request, created_on) VALUES (?1, ?2, ?3)",
                    params![
                        username,
                        request.to_string(),
                        chrono::Utc::now().to_rfc3339()
                    ],
                )
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let count: i64 = transaction
                .query_row(
                    "SELECT COUNT(*) FROM visits WHERE username = ?1",
                    [&username],
                    |row| row.get(0),
                )
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let image = state.image(count)?;
            transaction
                .commit()
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            Ok(([(header::CONTENT_TYPE, "image/png")], image).into_response())
        })
        .await
}
