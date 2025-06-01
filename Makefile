# Environment variables
PG_HOST ?= 127.0.0.1
PG_USER ?= postgres
PG_PASSWORD ?= postgres
PG_DATABASE ?= demo_1gb
S3_BUCKET ?= pgbackup
S3_ENDPOINT ?= http://127.0.0.1:9000
S3_ACCESS_KEY ?= minioadmin
S3_SECRET_KEY ?= minioadmin
PASSPHRASE ?= test-passphrase

# Docker commands
.PHONY: up down restart clean init-dbs

up:
	docker compose -f docker/compose.yml up -d

down:
	docker compose -f docker/compose.yml down

restart: down up

clean: down
	rm -rf postgres_data minio_data pgadmin_data

# Database initialization
init-dbs:
	@echo "Creating demo databases..."
	@docker exec docker-postgres-1 psql -U postgres -c "CREATE DATABASE demo_10mb;" || true
	@docker exec docker-postgres-1 psql -U postgres -c "CREATE DATABASE demo_20mb;" || true
	@docker exec docker-postgres-1 psql -U postgres -c "CREATE DATABASE demo_100mb;" || true
	@docker exec docker-postgres-1 psql -U postgres -c "CREATE DATABASE demo_1gb;" || true
	@echo "Populating demo_10mb..."
	@docker exec docker-postgres-1 psql -U postgres -d demo_10mb -c "CREATE TABLE IF NOT EXISTS sample_data (id SERIAL PRIMARY KEY, data TEXT, created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP); INSERT INTO sample_data (data) SELECT repeat(md5(random()::text), 100) FROM generate_series(1, 3000);"
	@echo "Populating demo_20mb..."
	@docker exec docker-postgres-1 psql -U postgres -d demo_20mb -c "CREATE TABLE IF NOT EXISTS sample_data (id SERIAL PRIMARY KEY, data TEXT, created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP); INSERT INTO sample_data (data) SELECT repeat(md5(random()::text), 100) FROM generate_series(1, 6000);"
	@echo "Populating demo_100mb..."
	@docker exec docker-postgres-1 psql -U postgres -d demo_100mb -c "CREATE TABLE IF NOT EXISTS sample_data (id SERIAL PRIMARY KEY, data TEXT, created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP); INSERT INTO sample_data (data) SELECT repeat(md5(random()::text), 100) FROM generate_series(1, 30000);"
	@echo "Populating demo_1gb..."
	@docker exec docker-postgres-1 psql -U postgres -d demo_1gb -c "CREATE TABLE IF NOT EXISTS sample_data (id SERIAL PRIMARY KEY, data TEXT, created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP); INSERT INTO sample_data (data) SELECT repeat(md5(random()::text), 100) FROM generate_series(1, 300000);"
	@echo "Database initialization complete!"

# Backup commands
.PHONY: backup list restore

backup:
	cargo run -- backup \
		--pg-host $(PG_HOST) \
		--pg-user $(PG_USER) \
		--pg-password $(PG_PASSWORD) \
		--pg-database $(PG_DATABASE) \
		--s3-bucket $(S3_BUCKET) \
		--aws-access-key-id $(S3_ACCESS_KEY) \
		--aws-secret-access-key $(S3_SECRET_KEY) \
		--aws-endpoint-url $(S3_ENDPOINT) \
		--passphrase $(PASSPHRASE) \
		--compression-enabled

list:
	cargo run -- list \
		--s3-bucket $(S3_BUCKET) \
		--aws-access-key-id $(S3_ACCESS_KEY) \
		--aws-secret-access-key $(S3_SECRET_KEY) \
		--aws-endpoint-url $(S3_ENDPOINT)

restore:
	cargo run -- restore \
		--pg-host $(PG_HOST) \
		--pg-user $(PG_USER) \
		--pg-password $(PG_PASSWORD) \
		--pg-database $(PG_DATABASE) \
		--s3-bucket $(S3_BUCKET) \
		--aws-access-key-id $(S3_ACCESS_KEY) \
		--aws-secret-access-key $(S3_SECRET_KEY) \
		--aws-endpoint-url $(S3_ENDPOINT) \
		--passphrase $(PASSPHRASE)

# Test scenarios
.PHONY: test-small test-medium test-large test-xlarge

test-small:
	PG_DATABASE=demo_10mb make backup

test-medium:
	PG_DATABASE=demo_20mb make backup

test-large:
	PG_DATABASE=demo_100mb make backup

test-xlarge:
	PG_DATABASE=demo_1gb make backup 