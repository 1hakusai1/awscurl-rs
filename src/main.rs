use std::{collections::HashMap, process::ExitCode, time::SystemTime};

use anyhow::{bail, Context};
use aws_config::SdkConfig;
use aws_credential_types::{provider::ProvideCredentials, Credentials};
use aws_sigv4::{
    http_request::{sign, SignableBody, SignableRequest, SigningSettings},
    sign::v4,
};
use chrono::{DateTime, FixedOffset};
use clap::{builder::ValueParser, Parser};
use sha2::{digest::FixedOutput, Digest, Sha256};

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

    #[arg(long, hide = true)]
    /// Print the request information instead of sending it
    /// Only for internal use
    dry_run: bool,

    #[arg(long, hide = true,value_parser = ValueParser::new(parse_datetime))]
    /// Fix the datetime
    /// Only for internal use
    datetime: Option<DateTime<FixedOffset>>,
}

fn parse_datetime(raw: &str) -> Result<DateTime<FixedOffset>, chrono::ParseError> {
    DateTime::parse_from_rfc3339(raw)
}

struct AwsCurlParam {
    args: Args,
    config: SdkConfig,
}
const DEFAULT_SERVICE: &str = "execute-api";

impl AwsCurlParam {
    fn new(args: Args, config: SdkConfig) -> Self {
        Self { args, config }
    }

    fn time(&self) -> SystemTime {
        self.args
            .datetime
            .map(SystemTime::from)
            .unwrap_or(SystemTime::now())
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

    async fn build_request(&self) -> anyhow::Result<http::Request<String>> {
        let args: &Args = &self.args;
        let mut builder = http::Request::builder();
        for (key, value) in self.headers()? {
            builder = builder.header(key, value);
        }

        // Generate x-amz-content-sha256 header automatically
        let body = self.args.data.as_deref().unwrap_or("");
        let body_hash = calc_sha256_hex_digest(body);
        builder = builder.header("x-amz-content-sha256", body_hash);

        let mut req = builder
            .uri(args.url.clone())
            .method(self.method().as_bytes())
            .body(body.to_string())?;

        let identity = self.credentials().await?.into();
        let signing_params = v4::SigningParams::builder()
            .identity(&identity)
            .time(self.time())
            .settings(SigningSettings::default())
            .region(self.region()?)
            .name(self.service())
            .build()?
            .into();
        let signable_request = SignableRequest::new(
            req.method().as_str(),
            req.uri().to_string(),
            req.headers()
                .iter()
                .map(|(k, v)| (k.as_str(), std::str::from_utf8(v.as_bytes()).unwrap())),
            SignableBody::Bytes(req.body().as_bytes()),
        )?;
        let (instruction, _signature) = sign(signable_request, &signing_params)?.into_parts();

        instruction.apply_to_request_http1x(&mut req);
        Ok(req)
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let status = inner().await.unwrap_or_else(|e| {
        eprintln!("{:?}", e);
        ExitCode::FAILURE
    });
    status
}

async fn inner() -> anyhow::Result<ExitCode> {
    let args = Args::parse();
    let mut config_loader = aws_config::from_env();
    if let Some(profile) = &args.profile {
        config_loader = config_loader.profile_name(profile);
    }
    let config = config_loader.load().await;
    let param = AwsCurlParam::new(args, config);

    let req = param.build_request().await?.try_into()?;
    if param.args.verbose {
        print_request_verbose(&req);
    }
    if param.args.dry_run {
        return Ok(ExitCode::SUCCESS);
    }

    let res = reqwest::Client::new().execute(req).await?;
    if param.args.verbose {
        print_response_verbose(&res);
    }

    let status = res.status();
    let body = res.text().await?;
    println!("{}", body);
    if status.is_success() {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::FAILURE)
    }
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

fn calc_sha256_hex_digest(body: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(body.as_bytes());
    hex::encode(hasher.finalize_fixed())
}

#[cfg(test)]
mod tests {

    use std::{collections::HashMap, process::Command};

    use aws_config::{Region, SdkConfig};
    use aws_credential_types::{provider::SharedCredentialsProvider, Credentials};
    use insta_cmd::{assert_cmd_snapshot, get_cargo_bin};

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
            dry_run: false,
            datetime: None,
        };
        let param = AwsCurlParam::new(args, generate_config("", "", None));
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
            dry_run: false,
            datetime: None,
        };
        let param = AwsCurlParam::new(args, generate_config("", "", None));
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
            dry_run: false,
            datetime: None,
        };
        let param = AwsCurlParam::new(args, generate_config("", "", None));
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
            dry_run: false,
            datetime: None,
        };
        let param = AwsCurlParam::new(args, generate_config("", "", None));
        assert_eq!(param.method(), "POST")
    }

    static TEST_ENV: [(&str, &str); 3] = [
        ("AWS_ACCESS_KEY_ID", "AKIAIOSFODNN7EXAMPLE"),
        (
            "AWS_SECRET_ACCESS_KEY",
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
        ),
        ("AWS_DEFAULT_REGION", "us-east-1"),
    ];

    static TEST_ARGS: [&str; 4] = [
        "--dry-run",
        "--verbose",
        "--datetime",
        "2013-05-24T00:00:00Z",
    ];

    #[test]
    fn get_request() {
        // Same as https://docs.aws.amazon.com/AmazonS3/latest/API/sig-v4-header-based-auth.html
        assert_cmd_snapshot!(Command::new(get_cargo_bin("awscurl")).envs(TEST_ENV).args(TEST_ARGS).args([
            "https://examplebucket.s3.amazonaws.com/test.txt",
            "-H", "Range: bytes=0-9",
            "--service", "s3",
        ]), @"");
    }

    #[test]
    fn post_request_with_header() {
        // Same as https://docs.aws.amazon.com/AmazonS3/latest/API/sig-v4-header-based-auth.html
        assert_cmd_snapshot!(Command::new(get_cargo_bin("awscurl")).envs(TEST_ENV).args(TEST_ARGS).args([
            "https://examplebucket.s3.amazonaws.com/test$file.text",
            "-X", "PUT",
            "-H", "Date: Fri, 24 May 2013 00:00:00 GMT",
            "-H", "x-amz-storage-class: REDUCED_REDUNDANCY",
            "-d", "Welcome to Amazon S3.",
            "--service", "s3",
        ]), @"");
    }
}
