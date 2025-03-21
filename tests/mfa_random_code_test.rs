use std::{net::SocketAddr, thread};

use actix_session::{storage::CookieSessionStore, SessionExt, SessionMiddleware};
use actix_web::{cookie::Key, get, App, HttpRequest, HttpResponse, HttpServer, Responder};
use authfix::{
    middleware::{AuthMiddleware, PathMatcher},
    multifactor::random_code_auth::{CodeSender, MfaRandomCode, RandomCode},
    session::{
        handlers::{login_config, SessionLoginHandler},
        session_auth::SessionAuthProvider,
    },
    AuthToken,
};
use chrono::{DateTime, Duration, Local, TimeDelta};
use reqwest::{Client, StatusCode};
use test_utils::{CustomError, HardCodedLoadUserService, User};

mod test_utils;

#[actix_rt::test]
async fn should_be_able_to_logout() {
    let addr = actix_test::unused_addr();
    start_test_server(addr, single_code_generator);

    let client = Client::builder().cookie_store(true).build().unwrap();

    client
        .post(format!("http://{addr}/login"))
        .body("{ \"username\": \"anna\", \"password\": \"test123\" }")
        .header("Content-Type", "application/json")
        .send()
        .await
        .unwrap();

    client
        .post(format!("http://{addr}/login/mfa"))
        .body(format!("{{ \"code\": \"{}\" }}", "123abc"))
        .header("Content-Type", "application/json")
        .send()
        .await
        .unwrap();

    client
        .post(format!("http://{addr}/logout"))
        .send()
        .await
        .unwrap();

    let res = client
        .get(format!("http://{addr}/secured-route"))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[actix_rt::test]
async fn should_be_possible_to_try_mfa_again() {
    let addr = actix_test::unused_addr();
    start_test_server(addr, single_code_generator);

    let client = Client::builder().cookie_store(true).build().unwrap();

    client
        .post(format!("http://{addr}/login"))
        .body("{ \"username\": \"anna\", \"password\": \"test123\" }")
        .header("Content-Type", "application/json")
        .send()
        .await
        .unwrap();

    client
        .post(format!("http://{addr}/login/mfa"))
        .body(format!("{{ \"code\": \"{}\" }}", "oops wrong code"))
        .header("Content-Type", "application/json")
        .send()
        .await
        .unwrap();

    client
        .post(format!("http://{addr}/login/mfa"))
        .body(format!("{{ \"code\": \"{}\" }}", "123abc"))
        .header("Content-Type", "application/json")
        .send()
        .await
        .unwrap();

    let res = client
        .get(format!("http://{addr}/secured-route"))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

#[actix_rt::test]
async fn should_be_possible_to_login_again_before_mfa_has_been_passed() {
    let addr = actix_test::unused_addr();
    start_test_server(addr, single_code_generator);

    let client = Client::builder().cookie_store(true).build().unwrap();

    client
        .post(format!("http://{addr}/login"))
        .body("{ \"username\": \"anna\", \"password\": \"test123\" }")
        .header("Content-Type", "application/json")
        .send()
        .await
        .unwrap();

    // sets a value to annas login session
    client
        .get(format!("http://{addr}/unsecure/manipulate-session"))
        .send()
        .await
        .unwrap();

    // now, login with bob:
    client
        .post(format!("http://{addr}/login"))
        .body("{ \"username\": \"bob\", \"password\": \"test123\" }")
        .header("Content-Type", "application/json")
        .send()
        .await
        .unwrap();

    client
        .post(format!("http://{addr}/login/mfa"))
        .body(format!("{{ \"code\": \"{}\" }}", "123abc"))
        .header("Content-Type", "application/json")
        .send()
        .await
        .unwrap();

    let res = client
        .get(format!("http://{addr}/secured-route"))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    // Annas session attribute should not be available to Bobs session
    assert_eq!(
        res.text().await.unwrap(),
        "User: bob@example.org, privateValue: na"
    );
}

#[actix_rt::test]
async fn should_not_be_logged_in_if_code_is_wrong() {
    let addr = actix_test::unused_addr();
    start_test_server(addr, single_code_generator);

    let client = Client::builder().cookie_store(true).build().unwrap();

    client
        .post(format!("http://{addr}/login"))
        .body("{ \"username\": \"anna\", \"password\": \"test123\" }")
        .header("Content-Type", "application/json")
        .send()
        .await
        .unwrap();

    let res = client
        .get(format!("http://{addr}/secured-route"))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[actix_rt::test]
async fn should_be_logged_in_after_sending_correct_code() {
    let addr = actix_test::unused_addr();
    start_test_server(addr, single_code_generator);

    let client = Client::builder().cookie_store(true).build().unwrap();

    client
        .post(format!("http://{addr}/login"))
        .body("{ \"username\": \"anna\", \"password\": \"test123\" }")
        .header("Content-Type", "application/json")
        .send()
        .await
        .unwrap();

    client
        .post(format!("http://{addr}/login/mfa"))
        .body(format!("{{ \"code\": \"{}\" }}", "123abc"))
        .header("Content-Type", "application/json")
        .send()
        .await
        .unwrap();

    let res = client
        .get(format!("http://{addr}/secured-route"))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

#[actix_rt::test]
async fn should_not_be_logged_in_after_time_is_up() {
    let addr = actix_test::unused_addr();
    start_test_server(addr, immediately_not_valid_generator);

    let client = Client::builder().cookie_store(true).build().unwrap();

    client
        .post(format!("http://{addr}/login"))
        .body("{ \"username\": \"anna\", \"password\": \"test123\" }")
        .header("Content-Type", "application/json")
        .send()
        .await
        .unwrap();

    client
        .post(format!("http://{addr}/login/mfa"))
        .body(format!("{{ \"code\": \"{}\" }}", "123abc"))
        .header("Content-Type", "application/json")
        .send()
        .await
        .unwrap();

    let res = client
        .get(format!("http://{addr}/secured-route"))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

struct DummySender {}
impl CodeSender for DummySender {
    type Error = CustomError;

    fn send_code(&self, code: RandomCode) -> Result<(), Self::Error> {
        let st = code.valid_until().to_owned();
        let date_time: DateTime<Local> = st.into();
        let now = Local::now();
        let minutes = date_time.signed_duration_since(now).num_minutes() + 1; // +1 because the first minute is only a fraction
        println!(
            "Please enter code: {}, it is valid for {} minutes",
            code.value(),
            minutes
        );
        Ok(())
    }
}

#[get("/secured-route")]
pub async fn secured_route(token: AuthToken<User>, req: HttpRequest) -> impl Responder {
    let pv = req
        .get_session()
        .get::<String>("privateValue")
        .unwrap()
        .unwrap_or("na".to_owned());
    HttpResponse::Ok().body(format!(
        "User: {}, privateValue: {}",
        token.get_authenticated_user().email,
        pv
    ))
}

#[get("/unsecure/manipulate-session")]
pub async fn manipulate_session(req: HttpRequest) -> impl Responder {
    req.get_session()
        .insert("privateValue", "some value")
        .unwrap();
    HttpResponse::Ok()
}

fn create_actix_session_middleware() -> SessionMiddleware<CookieSessionStore> {
    let key = Key::generate();

    SessionMiddleware::new(CookieSessionStore::default(), key.clone())
}

fn single_code_generator() -> RandomCode {
    let valid_until = Local::now()
        .checked_add_signed(TimeDelta::minutes(5))
        .unwrap();
    RandomCode::new("123abc", valid_until.into())
}

fn immediately_not_valid_generator() -> RandomCode {
    let valid_until = Local::now() - Duration::minutes(1);
    RandomCode::new("123abc", valid_until.into())
}

fn start_test_server(addr: SocketAddr, generator: fn() -> RandomCode) {
    thread::spawn(move || {
        actix_rt::System::new()
            .block_on(async {
                HttpServer::new(move || {
                    App::new()
                        .service(secured_route)
                        .configure(login_config(SessionLoginHandler::with_mfa(
                            HardCodedLoadUserService {},
                        )))
                        .wrap(AuthMiddleware::<_, User>::new_with_factor(
                            SessionAuthProvider,
                            PathMatcher::new(vec!["/login", "/unsecure/*"], true),
                            Box::new(MfaRandomCode::new(generator, DummySender {})),
                        ))
                        .wrap(create_actix_session_middleware())
                })
                .bind(format!("{addr}"))
                .unwrap()
                .run()
                .await
            })
            .unwrap();
    });
}
