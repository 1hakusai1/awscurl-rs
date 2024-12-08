# awscurl-rs

Implementation of [awscurl](https://github.com/okigan/awscurl) written in Rust.

## Usage

```
export AWS_PROFILE=your-profile
awscurl https://example.com
```

```
export AWS_ACCESS_KEY_ID=YOURACCESSKEYID
export AWS_SECRET_ACCESS_KEY=your/accesskey
export AWS_DEFAULT_REGION=ap-northeast-1
awscurl https://example.com
```

### Options

```
awscurl https://example.com -X POST -d '{"example": "value"}' -H 'content-type: application/json' -H 'hoge: fuga'
```
