use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use serde_json::Value;
use tower::ServiceExt;
use visit_count_generator::{AppState, router};

async fn get(app: axum::Router, path: &str) -> (u16, axum::http::HeaderMap, Vec<u8>) {
    let response = app
        .oneshot(
            Request::builder()
                .uri(path)
                .header("host", "localhost")
                .header("x-forwarded-for", "192.0.2.1")
                .header("cookie", "test=value")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec();
    (status, headers, bytes)
}

#[tokio::test]
async fn endpoints_preserve_counts_metadata_and_persistence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("visits.sqlite3");
    let app = router(AppState::open(&path).unwrap());
    let (_, _, bytes) = get(app.clone(), "/visit-count/query/missing").await;
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap(),
        serde_json::json!([])
    );
    let (status, headers, first) =
        get(app.clone(), "/visit-count/generate/alice.png?source=readme").await;
    assert_eq!(status, 200);
    assert_eq!(headers["content-type"], "image/png");
    assert!(
        headers["cache-control"]
            .to_str()
            .unwrap()
            .contains("no-store")
    );
    let png = image::load_from_memory(&first).unwrap().to_rgba8();
    assert_eq!(
        png.dimensions(),
        image::load_from_memory(include_bytes!("../VisitCount.png"))
            .unwrap()
            .to_rgba8()
            .dimensions()
    );
    assert_eq!(png.get_pixel(0, 0)[3], 0);
    let (_, _, second) = get(app.clone(), "/visit-count/generate/alice.png").await;
    assert_ne!(first, second);
    get(app.clone(), "/visit-count/generate/bob.png").await;
    let (_, _, bytes) = get(app.clone(), "/visit-count/query").await;
    let result: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(result["stats"]["alice"]["count"], 2);
    assert_eq!(result["stats"]["bob"]["count"], 1);
    assert_eq!(
        result["stats"]["alice"]["visitors"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let (_, _, bytes) = get(app.clone(), "/visit-count/query/alice").await;
    let visits: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(visits[0]["id"].as_i64() > visits[1]["id"].as_i64());
    assert_eq!(visits[1]["request"]["Query"][0], "source");
    assert_eq!(visits[1]["request"]["Cookies"][0]["Key"], "test");
    assert_eq!(
        get(app.clone(), "/visit-count/generate/alice.jpg").await.0,
        404
    );
    assert_eq!(get(app.clone(), "/visit-count/generate/.png").await.0, 404);
    drop(app);
    let reopened = router(AppState::open(path).unwrap());
    let (_, _, bytes) = get(reopened, "/visit-count/query/alice").await;
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes)
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn simultaneous_visits_are_not_lost() {
    let app = router(AppState::open(":memory:").unwrap());
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..12 {
        let app = app.clone();
        tasks.spawn(async move { get(app, "/visit-count/generate/concurrent.png").await.0 });
    }
    while let Some(result) = tasks.join_next().await {
        assert_eq!(result.unwrap(), 200);
    }
    let (_, _, bytes) = get(app, "/visit-count/query/concurrent").await;
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes)
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        12
    );
}
