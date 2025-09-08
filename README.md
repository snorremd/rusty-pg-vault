# rusty-pg-vault

A PostgreSQL backup tool that streams data directly to S3 storage without using local disk space.
The tool performs encryption and compression during the backup process, ensuring your data is both secure and efficiently stored.
Rusty PG Vault on top of `pg_dump` and `psql` to perform backups and restores.
It does not support incremental backups or write-ahead logs, opting instead to be as simple as possible.
You can run it anywhere as long as it can connect to your Postgres database and S3 endpoint.

## Installation

TBD

## Usage

The tool offers a suite of commands to help create and list backups, run restores, list databases and more.
Run the command with the help flag to get started.

```sh
rusty-pg-vault --help
```

### Configuration

Rusty PG Vault accepts configuration as environment variables or command line flags.

### Examples

Running backup with command line flags:

```sh
rusty-pg-vault backup create \
    --pg-host localhost \
    --pg-user postgres \
    --pg-password secret123 \
    --pg-database myapp \
    --s3-bucket my-backups \
    --aws-access-key-id AKIAXXXXXXXXXXXXXXXX \
    --aws-secret-access-key XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX \
    --aws-endpoint-url https://s3.amazonaws.com \
    --passphrase my-backup-encryption-key \
    --compression-enabled \
    --compression-level 10
```

Running backup with environment variables:

```sh
export PG_HOST=localhost
export PG_USER=postgres
export PG_PASSWORD=secret123
export PG_DATABASE=myapp
export S3_BUCKET=my-backups
export AWS_ACCESS_KEY_ID=AKIAXXXXXXXXXXXXXXXX
export AWS_SECRET_ACCESS_KEY=XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
export AWS_ENDPOINT_URL=https://s3.amazonaws.com
export PASSPHRASE=my-backup-encryption-key
export COMPRESSION_ENABLED=true
export COMPRESSION_LEVEL=10

rusty-pg-vault backup create
```

## Development
