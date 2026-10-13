use std::env;

use kasse::{auth, db, router, AppState};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tracing_subscriber::EnvFilter;

fn required_env(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("missing environment variable {name}"))
}

/// Passwords that ship in `.env.example` must never be used for real.
fn is_placeholder(password: &str) -> bool {
    password.starts_with("change-me")
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "kasse=info,tower_http=info".into()),
        )
        .init();

    // The owner connection is only used for migrations and for setting up the app role.
    let database_url = required_env("DATABASE_URL");
    let owner = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await
        .expect("cannot connect to database");
    sqlx::migrate!("./migrations")
        .run(&owner)
        .await
        .expect("migrations failed");

    // CLI: `kasse create-user <username> [--admin]` reads the password from stdin.
    let args: Vec<String> = env::args().collect();
    if args.get(1).map(String::as_str) == Some("create-user") {
        create_user_cli(&owner, &args[2..]).await;
        return;
    }

    let pool = runtime_pool(&owner).await;
    owner.close().await;

    bootstrap_admin(&pool).await;
    match kasse::seed::run(&pool).await {
        Ok(0) => {}
        Ok(n) => tracing::info!("seeded {n} products from the price list"),
        Err(e) => panic!("seeding failed: {e}"),
    }

    let cookie_secure = env::var("COOKIE_SECURE")
        .map(|v| v == "true")
        .unwrap_or(false);
    let state = AppState::new(pool, cookie_secure).expect("cannot initialise state");
    let bind = env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .expect("cannot bind address");
    tracing::info!("listening on {bind}");
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
    tracing::info!("stopped");
}

/// Resolves on SIGTERM (`docker stop`) or Ctrl-C. As process 1 of a container the app would
/// otherwise ignore SIGTERM and be killed after 10 seconds; this lets running requests finish.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutting down");
}

/// Pool for serving requests. With `APP_DB_USER`/`APP_DB_PASSWORD` the app runs as a restricted
/// role (no schema changes, ledger tables append-only); without them it falls back to the owner.
async fn runtime_pool(owner: &PgPool) -> PgPool {
    let (Ok(user), Ok(password)) = (env::var("APP_DB_USER"), env::var("APP_DB_PASSWORD")) else {
        tracing::warn!(
            "APP_DB_USER/APP_DB_PASSWORD not set: the app runs with the database owner's rights"
        );
        return owner.clone();
    };
    assert!(
        !is_placeholder(&password),
        "APP_DB_PASSWORD still has the placeholder value from .env.example"
    );
    db::roles::ensure_app_role(owner, &user, &password)
        .await
        .expect("cannot set up the database role");
    let options: PgConnectOptions = owner
        .connect_options()
        .as_ref()
        .clone()
        .username(&user)
        .password(&password);
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect_with(options)
        .await
        .expect("cannot connect as the app role");
    tracing::info!("running with restricted database role '{user}'");
    pool
}

/// Creates the first admin from ADMIN_USER / ADMIN_PASSWORD, only if no user exists yet.
async fn bootstrap_admin(pool: &PgPool) {
    let count = db::users::count(pool).await.expect("cannot count users");
    if count > 0 {
        return;
    }
    let (Ok(user), Ok(password)) = (env::var("ADMIN_USER"), env::var("ADMIN_PASSWORD")) else {
        tracing::warn!("no users exist and ADMIN_USER/ADMIN_PASSWORD are not set");
        return;
    };
    assert!(
        password.chars().count() >= 8,
        "ADMIN_PASSWORD must have at least 8 characters"
    );
    assert!(
        !is_placeholder(&password),
        "ADMIN_PASSWORD still has the placeholder value from .env.example"
    );
    let hash = auth::hash_password(&password).expect("cannot hash admin password");
    db::users::create_with_role(pool, user.trim(), &hash, true)
        .await
        .expect("cannot create admin");
    tracing::info!("created admin user '{user}'");
}

async fn create_user_cli(pool: &PgPool, args: &[String]) {
    let is_admin = args.iter().any(|a| a == "--admin");
    let Some(username) = args.iter().find(|a| !a.starts_with("--")) else {
        eprintln!("usage: kasse create-user <username> [--admin]   (password via stdin)");
        std::process::exit(2);
    };
    let mut password = String::new();
    std::io::stdin()
        .read_line(&mut password)
        .expect("cannot read password");
    let password = password.trim_end_matches(['\r', '\n']);
    if password.chars().count() < 8 {
        eprintln!("password must have at least 8 characters");
        std::process::exit(2);
    }
    let hash = auth::hash_password(password).expect("cannot hash password");
    match db::users::create_with_role(pool, username.trim(), &hash, is_admin).await {
        Ok(_) => println!(
            "user '{username}' created{}",
            if is_admin { " (admin)" } else { "" }
        ),
        Err(e) => {
            eprintln!("could not create user: {e}");
            std::process::exit(1);
        }
    }
}
