# awscurl-rs

Implementation of [awscurl](https://github.com/okigan/awscurl) written in Rust.

## Installation

### Homebrew

```
brew tap 1hakusai1/1hakusai1
brew install awscurl-rs
```

### From source

```
git clone git@github.com:1hakusai1/awscurl-rs.git
cd awscurl-rs
cargo install
```

## Usage

### Use profile

```
export AWS_PROFILE=your-profile
awscurl https://example.com
```

### Use access key

```
export AWS_ACCESS_KEY_ID=YOURACCESSKEYID
export AWS_SECRET_ACCESS_KEY=your/accesskey
export AWS_DEFAULT_REGION=ap-northeast-1
awscurl https://example.com
```

### Assume role

Config file

```
[profile source-profile-name]
region = ap-northeast-1

[profile assume-role-profile]
region = ap-northeast-1
role_arn = arn:aws:iam::000000000000:role/role-name
source_profile = source-profile-name
```

```
export AWS_PROFILE=assume-role-profile
awscurl https://example.com
```

## Example

Most of the options are the same as those in curl.

```
awscurl https://example.com -X POST -d '{"example": "value"}' -H 'content-type: application/json' -H 'hoge: fuga' -v
```

## Options

```
> awscurl --help
Usage: awscurl [OPTIONS] <URL>

Arguments:
  <URL>

Options:
  -d, --data <DATA>
  -X, --request <METHOD>
  -H, --header <HEADER>
  -v, --verbose
  -h, --help              Print help
  -V, --version           Print version
```
