# awscurl-rs

Implementation of [awscurl](https://github.com/okigan/awscurl) written in Rust.

## Installation

### Homebrew

```shell
brew tap 1hakusai1/1hakusai1
brew install awscurl-rs
```

### From source

```shell
git clone git@github.com:1hakusai1/awscurl-rs.git
cd awscurl-rs
cargo install
```

## Usage

### Use profile

```shell
AWS_PROFILE=your-profile awscurl https://example.com
# or
awscurl https://example.com --profile your-profile
```

### Use access key

```shell
AWS_ACCESS_KEY_ID=YOURACCESSKEYID AWS_SECRET_ACCESS_KEY=your/accesskey AWS_DEFAULT_REGION=ap-northeast-1 awscurl https://example.com
```

### Assume role

```
[profile source-profile-name]
region = ap-northeast-1

[profile assume-role-profile]
region = ap-northeast-1
role_arn = arn:aws:iam::000000000000:role/role-name
source_profile = source-profile-name
```

```shell
awscurl https://example.com --profile assume-role-profile
```

## Example
S3 list bucket content
```shell
awscurl --service s3 'https://hakusai-test-bucket.s3.amazonaws.com' | tidy -xml -iq
<?xml version="1.0" encoding="utf-8"?>
<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Name>hakusai-test-bucket</Name>
  <Prefix></Prefix>
  <Marker></Marker>
  <MaxKeys>1000</MaxKeys>
  <IsTruncated>false</IsTruncated>
</ListBucketResult>
```

Most of the options are the same as those in curl.

```shell
awscurl https://example.com -X POST -d '{"example": "value"}' -H 'content-type: application/json' -H 'hoge: fuga' -v
```

## Options

```
> awscurl --help
Usage: awscurl [OPTIONS] <URL>

Arguments:
  <URL>

Options:
  -d, --data <DATA>        Request body
  -X, --request <METHOD>   HTTP method (Ex. GET, POST, PUT ...)
  -H, --header <HEADER>    HTTP headers (Ex. content-type: application/json)
      --service <SERVICE>  AWS service name (Default: execute-api)
      --region <REGION>    AWS region
      --profile <PROFILE>  AWS profile
  -v, --verbose
  -h, --help               Print help
  -V, --version            Print version
```
