//! End-to-end through the HTTP layer: login, stock management, booking, statistics.
mod common;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::response::Response;
use axum::Router;
use http_body_util::BodyExt;
use kasse::{auth, db, router, AppState};
use sqlx::PgPool;
use tower::ServiceExt;

struct Client {
    app: Router,
    session: String,
    csrf: String,
}

async fn text(res: Response) -> String {
    String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap()
}

fn set_cookie(res: &Response, name: &str) -> Option<String> {
    res.headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with(&format!("{name}=")))
        .map(|v| v.split(';').next().unwrap().to_string())
}

fn extract_csrf(html: &str) -> String {
    let marker = "name=\"csrf_token\" value=\"";
    let start = html.find(marker).expect("csrf field") + marker.len();
    html[start..].split('"').next().unwrap().to_string()
}

fn form(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={}", kasse::routes::urlencode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

impl Client {
    async fn login(pool: PgPool) -> Client {
        let hash = auth::hash_password("geheim123").unwrap();
        db::users::create_with_role(&pool, "admin", &hash, true)
            .await
            .unwrap();
        let app = router(AppState::new(pool, false).unwrap());

        let res = app
            .clone()
            .oneshot(Request::get("/login").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let login_cookie = set_cookie(&res, auth::LOGIN_CSRF_COOKIE).unwrap();
        let token = login_cookie.split('=').nth(1).unwrap().to_string();
        let req = Request::post("/login")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, &login_cookie)
            .body(Body::from(form(&[
                ("username", "admin"),
                ("password", "geheim123"),
                ("csrf_token", &token),
            ])))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::SEE_OTHER);
        let session = set_cookie(&res, auth::SESSION_COOKIE).unwrap();

        let mut c = Client {
            app,
            session,
            csrf: String::new(),
        };
        let home = text(c.get("/").await).await;
        c.csrf = extract_csrf(&home);
        c
    }

    async fn get(&self, uri: &str) -> Response {
        let req = Request::get(uri)
            .header(header::COOKIE, &self.session)
            .body(Body::empty())
            .unwrap();
        self.app.clone().oneshot(req).await.unwrap()
    }

    async fn post_with(&self, uri: &str, csrf: &str, pairs: &[(&str, &str)]) -> Response {
        let mut all = vec![("csrf_token", csrf)];
        all.extend_from_slice(pairs);
        let req = Request::post(uri)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, &self.session)
            .body(Body::from(form(&all)))
            .unwrap();
        self.app.clone().oneshot(req).await.unwrap()
    }

    async fn post(&self, uri: &str, pairs: &[(&str, &str)]) -> Response {
        self.post_with(uri, &self.csrf, pairs).await
    }
}

/// Logs in as `user` and returns a client for that session, or `None` if the login fails.
async fn try_login(app: &Router, user: &str, password: &str) -> Option<Client> {
    let res = app
        .clone()
        .oneshot(Request::get("/login").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let login_cookie = set_cookie(&res, auth::LOGIN_CSRF_COOKIE).unwrap();
    let token = login_cookie.split('=').nth(1).unwrap().to_string();
    let req = Request::post("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, &login_cookie)
        .body(Body::from(form(&[
            ("username", user),
            ("password", password),
            ("csrf_token", &token),
        ])))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let session = set_cookie(&res, auth::SESSION_COOKIE)?;
    Some(Client {
        app: app.clone(),
        session,
        csrf: String::new(),
    })
}

async fn login_as(app: &Router, user: &str, password: &str) -> Client {
    try_login(app, user, password).await.expect("login")
}

fn location(res: &Response) -> String {
    res.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_string()
}

#[sqlx::test]
async fn booking_flow_from_cart_to_confirmation(pool: PgPool) {
    let c = Client::login(pool.clone()).await;
    let user: i64 = sqlx::query_scalar("SELECT id FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    let cat = db::categories::create(&pool, "Alkohol").await.unwrap();
    let beer = db::products::create(&pool, cat, "Bier", 300, false)
        .await
        .unwrap();
    let empty = db::products::create(&pool, cat, "Radler", 300, false)
        .await
        .unwrap();
    db::stock::adjust(&pool, beer, 5, "Wareneingang", None, user)
        .await
        .unwrap();

    // category page and tiles; sold-out product is visible but disabled
    let html = text(c.get("/buchen").await).await;
    assert!(html.contains("Alkohol") && html.contains("Alle"));
    let html = text(c.get(&format!("/buchen/{cat}")).await).await;
    assert!(html.contains("Bier") && html.contains("3,00 €"));
    assert!(html.contains("Ausverkauft"));
    assert!(html.contains("disabled"), "sold out tile is disabled");
    assert!(html.contains("Noch nichts im Warenkorb"));

    // search inside the tiles
    let html = text(c.get("/buchen/alle?q=bie").await).await;
    assert!(html.contains("Bier") && !html.contains("Radler"));

    // adding: twice beer, sold-out radler is ignored
    let back = format!("/buchen/{cat}");
    for _ in 0..2 {
        let res = c
            .post(&format!("/buchen/warenkorb/{beer}/add"), &[("back", &back)])
            .await;
        assert_eq!(res.status(), StatusCode::SEE_OTHER);
        assert_eq!(location(&res), back);
    }
    c.post(
        &format!("/buchen/warenkorb/{empty}/add"),
        &[("back", &back)],
    )
    .await;
    let html = text(c.get(&back).await).await;
    assert!(html.contains("6,00 €"), "total of two beers");
    assert!(
        !html.contains("Radler</div>"),
        "sold out product not in cart"
    );

    // +/- and remove
    c.post(
        &format!("/buchen/warenkorb/{beer}/plus"),
        &[("back", &back)],
    )
    .await;
    c.post(
        &format!("/buchen/warenkorb/{beer}/minus"),
        &[("back", &back)],
    )
    .await;
    let html = text(c.get(&back).await).await;
    assert!(html.contains("6,00 €"));

    // open redirect is blocked
    let res = c
        .post(
            &format!("/buchen/warenkorb/{beer}/plus"),
            &[("back", "https://evil.example")],
        )
        .await;
    assert_eq!(location(&res), "/buchen");
    c.post(
        &format!("/buchen/warenkorb/{beer}/minus"),
        &[("back", &back)],
    )
    .await;

    // payment page and live change fragment
    let html = text(c.get("/buchen/abbuchen").await).await;
    assert!(html.contains("6,00 €") && html.contains("Gegeben") && html.contains("Passend"));
    // typing re-requests the pay page with the amount; the change shows up in the same page
    let frag = text(c.get("/buchen/abbuchen?gegeben=10").await).await;
    assert!(
        frag.contains("4,00 €") && frag.contains("Rückgeld"),
        "{frag}"
    );
    let frag = text(c.get("/buchen/abbuchen?gegeben=5").await).await;
    assert!(
        frag.contains("Es fehlen") && frag.contains("1,00 €"),
        "{frag}"
    );
    let frag = text(c.get("/buchen/abbuchen?gegeben=6").await).await;
    assert!(frag.contains("0,00 €"), "exact amount: {frag}");

    // too little money: 422, nothing booked, cart stays
    let res = c.post("/buchen/abbuchen", &[("gegeben", "5")]).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text(res).await.contains("reicht nicht"));
    assert_eq!(common::count(&pool, "sales").await, 0);
    assert_eq!(common::stock_of(&pool, beer).await, 5);
    let res = c.post("/buchen/abbuchen", &[("gegeben", "abc")]).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        text(c.get(&back).await).await.contains("6,00 €"),
        "cart survived the failed attempts"
    );

    // successful booking
    let res = c.post("/buchen/abbuchen", &[("gegeben", "10")]).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let confirm = location(&res);
    assert!(confirm.starts_with("/buchen/fertig/"));
    assert_eq!(common::stock_of(&pool, beer).await, 3);
    let html = text(c.get(&confirm).await).await;
    assert!(
        html.contains("Verkauf abgeschlossen")
            && html.contains("6,00 €")
            && html.contains("10,00 €")
            && html.contains("4,00 €")
    );
    assert!(html.contains("2× Bier"));

    // cart is empty now; paying again redirects instead of booking twice
    let res = c.post("/buchen/abbuchen", &[("gegeben", "10")]).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&res), "/buchen");
    assert_eq!(common::count(&pool, "sales").await, 1);
    assert_eq!(location(&c.get("/buchen/abbuchen").await), "/buchen");

    // statistics and home show the sale
    let html = text(c.get("/statistik").await).await;
    assert!(html.contains("Bier") && html.contains("6,00 €") && html.contains("Alkohol"));
    let html = text(c.get("/").await).await;
    assert!(html.contains("6,00 €"));
}

#[sqlx::test]
async fn cart_stops_at_available_stock_and_checkout_rechecks(pool: PgPool) {
    let c = Client::login(pool.clone()).await;
    let user: i64 = sqlx::query_scalar("SELECT id FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    let cat = db::categories::create(&pool, "Alkohol").await.unwrap();
    let beer = db::products::create(&pool, cat, "Bier", 300, false)
        .await
        .unwrap();
    db::stock::adjust(&pool, beer, 2, "Wareneingang", None, user)
        .await
        .unwrap();

    for _ in 0..5 {
        c.post(
            &format!("/buchen/warenkorb/{beer}/add"),
            &[("back", "/buchen/alle")],
        )
        .await;
    }
    let html = text(c.get("/buchen/alle").await).await;
    assert!(html.contains("6,00 €"), "never more than the 2 in stock");

    // somebody else removes stock in the meantime
    db::stock::adjust(&pool, beer, -1, "Korrektur", None, user)
        .await
        .unwrap();
    let res = c.post("/buchen/abbuchen", &[("gegeben", "10")]).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text(res).await.contains("Nicht genug Bestand"));
    assert_eq!(common::stock_of(&pool, beer).await, 1);
    assert_eq!(common::count(&pool, "sales").await, 0);
    assert!(
        text(c.get("/buchen/alle").await).await.contains("6,00 €"),
        "cart is restored after a failed booking"
    );
}

#[sqlx::test]
async fn post_routes_require_a_valid_csrf_token(pool: PgPool) {
    let c = Client::login(pool.clone()).await;
    let cat = db::categories::create(&pool, "Alkohol").await.unwrap();
    let beer = db::products::create(&pool, cat, "Bier", 300, false)
        .await
        .unwrap();

    for uri in [
        format!("/buchen/warenkorb/{beer}/add"),
        "/buchen/abbuchen".to_string(),
        "/lager/produkte/neu".to_string(),
        format!("/lager/produkte/{beer}/bearbeiten"),
        format!("/lager/produkte/{beer}/loeschen"),
        format!("/lager/produkte/{beer}/bestand"),
        "/lager/kategorien".to_string(),
        "/admin/benutzer".to_string(),
        "/admin/benutzer/1/passwort".to_string(),
        "/admin/benutzer/1/status".to_string(),
        "/admin/benutzer/1/rolle".to_string(),
        format!("/lager/kategorien/{cat}/umbenennen"),
        format!("/lager/kategorien/{cat}/loeschen"),
    ] {
        let res = c
            .post_with(
                &uri,
                "wrong",
                &[
                    ("name", "x"),
                    ("price", "1"),
                    ("category_id", "1"),
                    ("gegeben", "1"),
                    ("mode", "add"),
                    ("quantity", "1"),
                    ("username", "x"),
                    ("active", "0"),
                    ("password", "x"),
                    ("admin", "1"),
                ],
            )
            .await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN, "{uri}");
    }
    assert!(db::products::get(&pool, beer).await.unwrap().is_some());
}

#[sqlx::test]
async fn stock_management_pages(pool: PgPool) {
    let c = Client::login(pool.clone()).await;

    // category
    let res = c.post("/lager/kategorien", &[("name", "Alkohol")]).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    for duplicate in ["Alkohol", " alkohol "] {
        let res = c.post("/lager/kategorien", &[("name", duplicate)]).await;
        assert_eq!(
            res.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{duplicate:?}"
        );
        assert!(text(res).await.contains("gibt es schon"));
    }
    let cat = db::categories::list(&pool).await.unwrap()[0].id;

    // product: invalid price re-renders the form, valid one is created with stock 0
    let res = c
        .post(
            "/lager/produkte/neu",
            &[
                ("name", "Pils"),
                ("price", "zwei"),
                ("category_id", &cat.to_string()),
            ],
        )
        .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(res).await;
    assert!(html.contains("gültigen Preis") && html.contains("value=\"Pils\""));
    let res = c
        .post(
            "/lager/produkte/neu",
            &[
                ("name", "Pils"),
                ("price", "2,5"),
                ("category_id", &cat.to_string()),
            ],
        )
        .await;
    assert_eq!(location(&res), "/lager?hinweis=angelegt");
    let product = db::products::list(&pool, None, None)
        .await
        .unwrap()
        .remove(0);
    assert_eq!((product.price_cents, product.stock), (250, 0));

    // list with notice, search and filter
    let html = text(c.get("/lager?hinweis=angelegt").await).await;
    assert!(html.contains("Produkt angelegt.") && html.contains("Pils") && html.contains("2,50 €"));
    assert!(!text(c.get("/lager?q=xyz").await).await.contains("Pils"));
    assert!(text(c.get(&format!("/lager?kategorie={cat}&q=pi")).await)
        .await
        .contains("Pils"));

    // edit page is prefilled
    let html = text(
        c.get(&format!("/lager/produkte/{}/bearbeiten", product.id))
            .await,
    )
    .await;
    assert!(html.contains("value=\"2,50\"") && html.contains("value=\"Pils\""));
    let res = c
        .post(
            &format!("/lager/produkte/{}/bearbeiten", product.id),
            &[
                ("name", "Pils neu"),
                ("price", "3"),
                ("category_id", &cat.to_string()),
            ],
        )
        .await;
    assert_eq!(location(&res), "/lager?hinweis=gespeichert");

    // add and remove stock; removing too much shows an error and keeps the form
    let uri = format!("/lager/produkte/{}/bestand", product.id);
    assert!(text(c.get(&uri).await).await.contains("Wareneingang"));
    let res = c
        .post(
            &uri,
            &[
                ("mode", "add"),
                ("quantity", "10"),
                ("comment", "Lieferung"),
            ],
        )
        .await;
    assert_eq!(location(&res), "/lager?hinweis=bestand");
    let res = c
        .post(&uri, &[("mode", "remove"), ("quantity", "11")])
        .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text(res).await.contains("nur 10 auf Lager"));
    let res = c
        .post(&uri, &[("mode", "remove"), ("quantity", "-3")])
        .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    c.post(
        &uri,
        &[("mode", "remove"), ("quantity", "4"), ("comment", "Bruch")],
    )
    .await;
    assert_eq!(common::stock_of(&pool, product.id).await, 6);
    let html = text(c.get("/lager/bewegungen").await).await;
    assert!(
        html.contains("Lieferung")
            && html.contains("Bruch")
            && html.contains("+10")
            && html.contains("-4")
    );

    // categories page, rename, delete rules
    let html = text(c.get("/lager/kategorien").await).await;
    assert!(html.contains("Alkohol"));
    c.post(
        &format!("/lager/kategorien/{cat}/umbenennen"),
        &[("name", "Bier & Wein")],
    )
    .await;
    assert!(
        text(c.get("/lager/kategorien").await)
            .await
            .contains("Bier &#38; Wein"),
        "output is escaped"
    );
    let res = c
        .post(&format!("/lager/kategorien/{cat}/loeschen"), &[])
        .await;
    assert_eq!(
        res.status(),
        StatusCode::UNPROCESSABLE_ENTITY,
        "category with products"
    );
    assert!(
        text(c.get(&format!("/lager/kategorien/{cat}/loeschen")).await)
            .await
            .contains("enthält noch 1 Produkt")
    );

    // soft delete: confirmation page, then gone from the list but still in the movements log
    assert!(text(
        c.get(&format!("/lager/produkte/{}/loeschen", product.id))
            .await
    )
    .await
    .contains("löschen?"));
    let res = c
        .post(&format!("/lager/produkte/{}/loeschen", product.id), &[])
        .await;
    assert_eq!(location(&res), "/lager?hinweis=geloescht");
    assert!(!text(c.get("/lager").await).await.contains("Pils neu"));
    assert!(text(c.get("/lager/bewegungen").await)
        .await
        .contains("Pils neu"));
    let res = c
        .post(&format!("/lager/kategorien/{cat}/loeschen"), &[])
        .await;
    assert_eq!(location(&res), "/lager/kategorien");
}

#[sqlx::test]
async fn statistics_page_handles_ranges_and_bad_input(pool: PgPool) {
    let c = Client::login(pool).await;
    for uri in [
        "/statistik",
        "/statistik?von=2026-01-01&bis=2026-12-31",
        "/statistik?von=kaputt&bis=",
    ] {
        let res = c.get(uri).await;
        assert_eq!(res.status(), StatusCode::OK, "{uri}");
        assert!(text(res).await.contains("Umsatz heute"));
    }
    // "from" after "to": the range becomes that one day, no error page
    let html = text(c.get("/statistik?von=2026-05-02&bis=2026-05-01").await).await;
    assert!(html.contains("02.05.2026") && !html.contains("01.05.2026 –"));
    assert!(html.contains("Keine Verkäufe im Zeitraum"));
}

#[sqlx::test]
async fn period_shortcuts_are_clickable_and_keep_their_highlight(pool: PgPool) {
    let c = Client::login(pool).await;
    // default: "Heute"
    let html = text(c.get("/statistik").await).await;
    assert_eq!(html.matches("class=\"chip active\"").count(), 1);
    assert!(html.contains("class=\"chip active\" href=\"/statistik?zeitraum=heute\""));
    // every shortcut highlights itself, even when its days equal another shortcut's (Monday: Heute == Woche)
    for key in ["heute", "gestern", "woche", "monat"] {
        let res = c.get(&format!("/statistik?zeitraum={key}")).await;
        assert_eq!(res.status(), StatusCode::OK, "{key}");
        let html = text(res).await;
        assert_eq!(html.matches("class=\"chip active\"").count(), 1, "{key}");
        assert!(
            html.contains(&format!(
                "class=\"chip active\" href=\"/statistik?zeitraum={key}\""
            )),
            "{key}"
        );
    }
    // a custom date range highlights nothing and keeps the given dates in the fields
    let html = text(c.get("/statistik?von=2026-01-01&bis=2026-01-02").await).await;
    assert_eq!(html.matches("class=\"chip active\"").count(), 0);
    assert!(html.contains("value=\"2026-01-01\"") && html.contains("value=\"2026-01-02\""));
    // unknown shortcut falls back to the default
    assert_eq!(
        c.get("/statistik?zeitraum=quatsch").await.status(),
        StatusCode::OK
    );
}

#[sqlx::test]
async fn admin_area_manages_users(pool: PgPool) {
    let c = Client::login(pool.clone()).await;
    let html = text(c.get("/admin").await).await;
    assert!(html.contains("admin") && html.contains("Neuer Benutzer") && html.contains("Du"));

    // validation
    let res = c
        .post(
            "/admin/benutzer",
            &[("username", "anna"), ("password", "kurz")],
        )
        .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text(res).await.contains("mindestens 8 Zeichen"));
    let res = c
        .post(
            "/admin/benutzer",
            &[("username", "  "), ("password", "langgenug123")],
        )
        .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let res = c
        .post(
            "/admin/benutzer",
            &[("username", "ADMIN"), ("password", "langgenug123")],
        )
        .await;
    assert_eq!(
        res.status(),
        StatusCode::UNPROCESSABLE_ENTITY,
        "names are unique ignoring case"
    );
    assert!(text(res).await.contains("gibt es schon"));

    // create, then log in as the new user
    let res = c
        .post(
            "/admin/benutzer",
            &[("username", "anna"), ("password", "langgenug123")],
        )
        .await;
    assert_eq!(location(&res), "/admin?hinweis=angelegt");
    assert!(text(c.get("/admin?hinweis=angelegt").await)
        .await
        .contains("Benutzer angelegt."));
    let anna_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'anna'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let anna = login_as(&c.app, "Anna", "langgenug123").await; // user names are matched ignoring case
    assert!(text(anna.get("/").await).await.contains("Abmelden"));

    // password change ends her sessions and the old password stops working
    let res = c
        .post(
            &format!("/admin/benutzer/{anna_id}/passwort"),
            &[("password", "neuesPasswort1")],
        )
        .await;
    assert_eq!(location(&res), "/admin?hinweis=passwort");
    assert_eq!(
        anna.get("/").await.status(),
        StatusCode::SEE_OTHER,
        "her old session is gone"
    );
    assert!(try_login(&c.app, "anna", "langgenug123").await.is_none());
    assert!(try_login(&c.app, "anna", "neuesPasswort1").await.is_some());

    // deactivate: no login, active sessions end; reactivate: works again
    let anna = login_as(&c.app, "anna", "neuesPasswort1").await;
    let res = c
        .post(
            &format!("/admin/benutzer/{anna_id}/status"),
            &[("active", "0")],
        )
        .await;
    assert_eq!(location(&res), "/admin?hinweis=deaktiviert");
    assert_eq!(anna.get("/").await.status(), StatusCode::SEE_OTHER);
    assert!(try_login(&c.app, "anna", "neuesPasswort1").await.is_none());
    assert!(text(c.get("/admin").await).await.contains("Deaktiviert"));
    c.post(
        &format!("/admin/benutzer/{anna_id}/status"),
        &[("active", "1")],
    )
    .await;
    assert!(try_login(&c.app, "anna", "neuesPasswort1").await.is_some());

    // roles: a regular user sees no admin tile and gets 403 on every admin route
    let anna = login_as(&c.app, "anna", "neuesPasswort1").await;
    let home = text(anna.get("/").await).await;
    assert!(
        !home.contains("href=\"/admin\""),
        "no admin tile for regular users"
    );
    let admin_home = text(c.get("/").await).await;
    assert!(
        admin_home.contains("href=\"/admin\""),
        "admins see the tile"
    );
    assert_eq!(anna.get("/admin").await.status(), StatusCode::FORBIDDEN);
    let anna_csrf = extract_csrf(&text(anna.get("/").await).await);
    for uri in [
        "/admin/benutzer".to_string(),
        format!("/admin/benutzer/{anna_id}/passwort"),
        format!("/admin/benutzer/{anna_id}/status"),
        format!("/admin/benutzer/{anna_id}/rolle"),
    ] {
        let res = anna
            .post_with(
                &uri,
                &anna_csrf,
                &[
                    ("username", "evil"),
                    ("password", "langgenug123"),
                    ("active", "1"),
                    ("admin", "1"),
                ],
            )
            .await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN, "{uri}");
    }
    let is_admin: bool = sqlx::query_scalar("SELECT is_admin FROM users WHERE username = 'anna'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!is_admin, "a regular user cannot promote themselves");

    // an admin can grant and revoke the role; it applies immediately
    let res = c
        .post(
            &format!("/admin/benutzer/{anna_id}/rolle"),
            &[("admin", "1")],
        )
        .await;
    assert_eq!(location(&res), "/admin?hinweis=admin");
    assert_eq!(
        anna.get("/admin").await.status(),
        StatusCode::OK,
        "same session, new rights"
    );
    let res = c
        .post(
            &format!("/admin/benutzer/{anna_id}/rolle"),
            &[("admin", "0")],
        )
        .await;
    assert_eq!(location(&res), "/admin?hinweis=kein_admin");
    assert_eq!(anna.get("/admin").await.status(), StatusCode::FORBIDDEN);
    // new user created with the admin checkbox
    c.post(
        "/admin/benutzer",
        &[
            ("username", "boss"),
            ("password", "langgenug123"),
            ("is_admin", "1"),
        ],
    )
    .await;
    let boss = login_as(&c.app, "boss", "langgenug123").await;
    assert_eq!(boss.get("/admin").await.status(), StatusCode::OK);

    // you cannot deactivate yourself or take away your own admin rights
    let me: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'admin'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let res = c
        .post(&format!("/admin/benutzer/{me}/status"), &[("active", "0")])
        .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let res = c
        .post(&format!("/admin/benutzer/{me}/rolle"), &[("admin", "0")])
        .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(c.get("/").await.status(), StatusCode::OK);
}

#[sqlx::test]
async fn all_app_pages_require_login(pool: PgPool) {
    let app = router(AppState::new(pool, false).unwrap());
    for uri in [
        "/",
        "/admin",
        "/lager",
        "/lager/produkte/neu",
        "/lager/bewegungen",
        "/lager/kategorien",
        "/buchen",
        "/buchen/alle",
        "/buchen/abbuchen",
        "/buchen/fertig/1",
        "/statistik",
    ] {
        let res = app
            .clone()
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::SEE_OTHER, "{uri}");
        assert_eq!(location(&res), "/login", "{uri}");
    }
}

#[sqlx::test]
async fn repeated_wrong_passwords_lock_the_login_for_a_while(pool: PgPool) {
    let c = Client::login(pool).await;
    for _ in 0..5 {
        assert!(try_login(&c.app, "admin", "falsch-falsch").await.is_none());
    }
    // locked now, even with the right password
    let res = c
        .app
        .clone()
        .oneshot(Request::get("/login").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let cookie = set_cookie(&res, auth::LOGIN_CSRF_COOKIE).unwrap();
    let token = cookie.split('=').nth(1).unwrap().to_string();
    let req = Request::post("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, &cookie)
        .body(Body::from(form(&[
            ("username", "ADMIN"),
            ("password", "geheim123"),
            ("csrf_token", &token),
        ])))
        .unwrap();
    let res = c.app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(set_cookie(&res, auth::SESSION_COOKIE).is_none());
    assert!(text(res).await.contains("Zu viele Fehlversuche"));
    // other users are not affected
    assert!(try_login(&c.app, "niemand", "x").await.is_none());
}

#[sqlx::test]
async fn responses_carry_security_headers(pool: PgPool) {
    let c = Client::login(pool).await;
    for uri in ["/login", "/", "/lager"] {
        let res = c.get(uri).await;
        let h = res.headers();
        let csp = h[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
        assert!(
            csp.contains("default-src 'self'")
                && csp.contains("frame-ancestors 'none'")
                && !csp.contains("unsafe"),
            "{uri}: {csp}"
        );
        assert_eq!(h[header::X_CONTENT_TYPE_OPTIONS], "nosniff", "{uri}");
        assert_eq!(h[header::X_FRAME_OPTIONS], "DENY", "{uri}");
        assert_eq!(h[header::REFERRER_POLICY], "same-origin", "{uri}");
        assert_eq!(
            h[header::CACHE_CONTROL],
            "no-store",
            "dynamic pages are never cached: {uri}"
        );
        assert!(
            h.get(header::STRICT_TRANSPORT_SECURITY).is_none(),
            "no HSTS over plain HTTP: {uri}"
        );
    }
    // static files may be cached and carry the same hardening
    let res = c.get("/static/app.css").await;
    assert!(res
        .headers()
        .get(header::CACHE_CONTROL)
        .is_none_or(|v| v != "no-store"));
    assert_eq!(res.headers()[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    // behind the HTTPS proxy the browser is told to keep using HTTPS
    let req = Request::get("/login")
        .header("x-forwarded-proto", "https")
        .body(Body::empty())
        .unwrap();
    let res = c.app.clone().oneshot(req).await.unwrap();
    assert!(res.headers()[header::STRICT_TRANSPORT_SECURITY]
        .to_str()
        .unwrap()
        .contains("max-age=31536000"));
}

#[sqlx::test]
async fn pages_do_not_need_inline_scripts_or_styles(pool: PgPool) {
    // The CSP forbids them, so the templates must not contain any.
    let c = Client::login(pool).await;
    for uri in [
        "/login",
        "/",
        "/lager",
        "/lager/kategorien",
        "/buchen",
        "/buchen/alle",
        "/statistik",
        "/admin",
        "/lager/produkte/neu",
    ] {
        let html = text(c.get(uri).await).await.to_lowercase();
        assert!(!html.contains("style=\""), "{uri} has an inline style");
        assert!(!html.contains("<style"), "{uri} has a style block");
        assert!(
            !html.contains(" onclick=") && !html.contains(" onchange="),
            "{uri} has inline handlers"
        );
        assert!(!html.contains("<script>"), "{uri} has an inline script");
        // htmx must not evaluate code (CSP + allowEval=false): no JS expressions in attributes
        assert!(
            !html.contains("morph:{") && !html.contains("hx-on") && !html.contains("js:"),
            "{uri} uses evaluated htmx config"
        );
        for m in html.match_indices("<script") {
            assert!(
                html[m.0..].starts_with("<script src=\"/static/"),
                "{uri}: only local scripts allowed"
            );
        }
    }
}

#[sqlx::test]
async fn oversized_request_bodies_are_rejected(pool: PgPool) {
    let c = Client::login(pool).await;
    let big = "x".repeat(200 * 1024);
    let res = c.post("/lager/kategorien", &[("name", &big)]).await;
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
}
