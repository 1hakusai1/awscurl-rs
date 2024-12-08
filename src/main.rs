use std::{env, time::SystemTime};

use aws_credential_types::provider::ProvideCredentials;
use aws_sigv4::{
    http_request::{sign, SignableBody, SignableRequest, SigningSettings},
    sign::v4,
};

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    let url = &args[1];
    let confg = aws_config::from_env().load().await;
    let identity = confg
        .credentials_provider()
        .unwrap()
        .provide_credentials()
        .await
        .unwrap()
        .into();
    let singing_settings = SigningSettings::default();
    let signing_params = v4::SigningParams::builder()
        .identity(&identity)
        .time(SystemTime::now())
        .settings(singing_settings)
        .region(confg.region().unwrap().as_ref())
        .name("execute-api")
        .build()
        .unwrap()
        .into();
    let signable_request =
        SignableRequest::new("GET", url, std::iter::empty(), SignableBody::Bytes(&[])).unwrap();
    let (instruction, _signature) = sign(signable_request, &signing_params)
        .unwrap()
        .into_parts();
    let mut req = http::Request::new("");
    instruction.apply_to_request_http1x(&mut req);
    println!("{:#?}", req);
    let res = reqwest::Client::new()
        .get(url)
        .headers(req.headers().clone())
        .body(req.body().to_string())
        .send()
        .await
        .unwrap();
    println!("{:#?}", res);
}
