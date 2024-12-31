use std::{
    collections::HashMap,
    process::{self},
    time::SystemTime,
};

use anyhow::{bail, Context};
use aws_config::SdkConfig;
use aws_credential_types::{provider::ProvideCredentials, Credentials};
use aws_sigv4::{
    http_request::{sign, SignableBody, SignableRequest, SigningSettings},
    sign::v4,
};
use clap::Parser;

#[derive(Parser, Debug)]
#[command(version)]
struct Args {
    url: String,

    #[arg(short, long)]
    /// Request body
    data: Option<String>,

    #[arg(short = 'X', long = "request")]
    /// HTTP method (Ex. GET, POST, PUT ...)
    method: Option<String>,

    #[arg(short = 'H', long)]
    /// HTTP headers (Ex. content-type: application/json)
    header: Vec<String>,

    #[arg(long)]
    /// AWS service name (Default: execute-api)
    service: Option<String>,

    #[arg(long)]
    /// AWS region
    region: Option<String>,

    #[arg(long)]
    /// AWS profile
    profile: Option<String>,

    #[arg(short, long)]
    verbose: bool,
}

struct AwsCurlParam {
    args: Args,
    config: SdkConfig,
    time: SystemTime,
}
const DEFAULT_SERVICE: &str = "execute-api";

impl AwsCurlParam {
    fn new(args: Args, config: SdkConfig, time: SystemTime) -> Self {
        Self { args, config, time }
    }

    fn service(&self) -> &str {
        self.args.service.as_deref().unwrap_or(DEFAULT_SERVICE)
    }

    fn region(&self) -> anyhow::Result<&str> {
        let config_region = self.config.region().map(|r| r.as_ref());
        self.args
            .region
            .as_deref()
            .or(config_region)
            .context("Unable to decide region")
    }

    fn method(&self) -> &str {
        // If the method is not specified and data is specified, POST method is used.
        // This behavior is same as curl.
        if let Some(method) = &self.args.method {
            return method.as_ref();
        }
        match self.args.data {
            Some(_) => "POST",
            None => "GET",
        }
    }

    fn headers(&self) -> anyhow::Result<HashMap<&str, &str>> {
        let mut ret = HashMap::new();
        for raw_string in self.args.header.iter() {
            let (key, value) = match raw_string.split_once(":") {
                Some(pair) => pair,
                None => bail!("Invalid header: {}", raw_string),
            };
            ret.insert(key.trim(), value.trim());
        }
        Ok(ret)
    }

    async fn credentials(&self) -> anyhow::Result<Credentials> {
        let config = self
            .config
            .credentials_provider()
            .context("Unable to find credentials")?
            .provide_credentials()
            .await?;
        Ok(config)
    }

    async fn build_request(&self) -> anyhow::Result<reqwest::Request> {
        let args: &Args = &self.args;
        let mut builder = http::Request::builder();
        for (key, value) in self.headers()? {
            builder = builder.header(key, value);
        }
        let mut unsigned_request = builder
            .uri(args.url.clone())
            .method(self.method().as_bytes())
            .body(args.data.clone().unwrap_or("".to_string()))?;

        let identity = self.credentials().await?.into();
        let signing_params = v4::SigningParams::builder()
            .identity(&identity)
            .time(self.time)
            .settings(SigningSettings::default())
            .region(self.region()?)
            .name(self.service())
            .build()?
            .into();
        let signable_request = SignableRequest::new(
            unsigned_request.method().as_str(),
            unsigned_request.uri().to_string(),
            unsigned_request
                .headers()
                .iter()
                .map(|(k, v)| (k.as_str(), std::str::from_utf8(v.as_bytes()).unwrap())),
            SignableBody::Bytes(unsigned_request.body().as_bytes()),
        )?;
        let (instruction, _signature) = sign(signable_request, &signing_params)?.into_parts();

        instruction.apply_to_request_http1x(&mut unsigned_request);
        let reqwest_req = unsigned_request.try_into()?;
        Ok(reqwest_req)
    }
}

#[tokio::main]
async fn main() {
    let status = inner().await.unwrap_or_else(|e| {
        eprintln!("{:?}", e);
        process::exit(1)
    });
    if !status.is_success() {
        process::exit(1)
    }
}

async fn inner() -> anyhow::Result<http::StatusCode> {
    let args = Args::parse();
    let mut config_loader = aws_config::from_env();
    if let Some(profile) = &args.profile {
        config_loader = config_loader.profile_name(profile);
    }
    let config = config_loader.load().await;
    let param = AwsCurlParam::new(args, config, SystemTime::now());

    let reqwest_req = param.build_request().await?;
    if param.args.verbose {
        print_request_verbose(&reqwest_req);
    }

    let res = reqwest::Client::new().execute(reqwest_req).await?;
    if param.args.verbose {
        print_response_verbose(&res);
    }

    let status = res.status();
    let body = res.text().await?;
    println!("{}", body);
    Ok(status)
}

fn print_request_verbose(req: &reqwest::Request) {
    eprintln!(
        "> {} {} {:?}",
        req.method().as_str(),
        req.url().path(),
        req.version()
    );
    for (key, value) in req.headers() {
        eprintln!("> {} {}", key.as_str(), value.to_str().unwrap())
    }
    eprintln!(">");
}

fn print_response_verbose(res: &reqwest::Response) {
    eprintln!("< {:?} {}", res.version(), res.status().as_str());
    for (key, value) in res.headers() {
        eprintln!("< {} {}", key.as_str(), value.to_str().unwrap())
    }
    eprintln!("<");
}

#[cfg(test)]
mod tests {

    use std::{collections::HashMap, time::SystemTime};

    use aws_config::{Region, SdkConfig};
    use aws_credential_types::{provider::SharedCredentialsProvider, Credentials};
    use chrono::{TimeZone, Utc};
    use insta::assert_debug_snapshot;

    use crate::{Args, AwsCurlParam};

    fn generate_config(
        access_key_id: &str,
        secret_access_key: &str,
        region: Option<&str>,
    ) -> SdkConfig {
        let credentials = Credentials::new(access_key_id, secret_access_key, None, None, "test");
        let provider = SharedCredentialsProvider::new(credentials);
        let mut config_builder = SdkConfig::builder().credentials_provider(provider);
        if let Some(region) = region {
            config_builder = config_builder.region(Region::new(region.to_string()));
        }
        config_builder.build()
    }

    #[test]
    fn parse_header() {
        let args = Args {
            url: "https://example.com".to_string(),
            data: None,
            method: None,
            header: vec![
                "content-type: application/json".to_string(),
                "referer: awscurl-rs".to_string(),
            ],
            service: None,
            region: None,
            profile: None,
            verbose: false,
        };
        let param = AwsCurlParam::new(args, generate_config("", "", None), SystemTime::now());
        assert_eq!(
            param.headers().unwrap(),
            HashMap::from([
                ("content-type", "application/json"),
                ("referer", "awscurl-rs")
            ])
        )
    }

    #[test]
    fn use_specified_method() {
        let args = Args {
            url: "https://example.com".to_string(),
            data: None,
            method: Some("PUT".to_string()),
            header: vec![],
            service: None,
            region: None,
            profile: None,
            verbose: false,
        };
        let param = AwsCurlParam::new(args, generate_config("", "", None), SystemTime::now());
        assert_eq!(param.method(), "PUT")
    }

    #[test]
    fn use_get_method_if_not_specified() {
        let args = Args {
            url: "https://example.com".to_string(),
            data: None,
            method: None,
            header: vec![],
            service: None,
            region: None,
            profile: None,
            verbose: false,
        };
        let param = AwsCurlParam::new(args, generate_config("", "", None), SystemTime::now());
        assert_eq!(param.method(), "GET")
    }

    #[test]
    fn use_post_method_if_data_is_specified() {
        let args = Args {
            url: "https://example.com".to_string(),
            data: Some("dummy data".to_string()),
            method: None,
            header: vec![],
            service: None,
            region: None,
            profile: None,
            verbose: false,
        };
        let param = AwsCurlParam::new(args, generate_config("", "", None), SystemTime::now());
        assert_eq!(param.method(), "POST")
    }

    #[tokio::test]
    async fn get_request() {
        let args = Args {
            url: "https://example.com".to_string(),
            data: None,
            method: None,
            header: vec![],
            service: None,
            region: None,
            profile: None,
            verbose: false,
        };
        let config = generate_config(
            "AKIAIOSFODNN7EXAMPLE",
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
            Some("ap-northeast-1"),
        );
        let time = Utc.with_ymd_and_hms(2013, 5, 24, 0, 0, 0).unwrap();
        let param = AwsCurlParam::new(args, config, time.into());
        let req = param.build_request().await.unwrap();
        assert_debug_snapshot!(req, @r#"
        Request {
            method: GET,
            url: Url {
                scheme: "https",
                cannot_be_a_base: false,
                username: "",
                password: None,
                host: Some(
                    Domain(
                        "example.com",
                    ),
                ),
                port: None,
                path: "/",
                query: None,
                fragment: None,
            },
            headers: {
                "x-amz-date": "20130524T000000Z",
                "authorization": "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/ap-northeast-1/execute-api/aws4_request, SignedHeaders=host;x-amz-date, Signature=3350cad4e732c60458fd6f31068a90a0179fcc63959c6891ccf3b7788b662c1d",
            },
        }
        "#)
    }

    #[tokio::test]
    async fn post_request_with_header() {
        let args = Args {
            url: "https://example.com".to_string(),
            data: Some(r#"{ "hoge": "fuga", "foo": "bar" }"#.to_string()),
            method: Some("POST".to_string()),
            header: vec!["content-type: application/json".to_string()],
            service: None,
            region: None,
            profile: None,
            verbose: false,
        };
        let config = generate_config(
            "AKIAIOSFODNN7EXAMPLE",
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
            Some("ap-northeast-1"),
        );
        let time = Utc.with_ymd_and_hms(2013, 5, 24, 0, 0, 0).unwrap();
        let param = AwsCurlParam::new(args, config, time.into());
        let req = param.build_request().await.unwrap();
        assert_debug_snapshot!(req, @r#"
        Request {
            method: POST,
            url: Url {
                scheme: "https",
                cannot_be_a_base: false,
                username: "",
                password: None,
                host: Some(
                    Domain(
                        "example.com",
                    ),
                ),
                port: None,
                path: "/",
                query: None,
                fragment: None,
            },
            headers: {
                "content-type": "application/json",
                "x-amz-date": "20130524T000000Z",
                "authorization": "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/ap-northeast-1/execute-api/aws4_request, SignedHeaders=content-type;host;x-amz-date, Signature=06cd3bf07213570b40e880f7c72ecded2ce70165134af7fc67a2c5ce21ea8b22",
            },
        }
        "#)
    }
}
