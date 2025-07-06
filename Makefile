# Environment variables
PG_HOST ?= 127.0.0.1
PG_USER ?= postgres
PG_PASSWORD ?= postgres
PG_DATABASE ?= demo_1gb
RESTORE_DATABASE ?= $(PG_DATABASE)_restored
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
.PHONY: backup backup-list backup-restore backup-and-restore restore-from databases

backup:
	cargo run -- backup create \
		--pg-host $(PG_HOST) \
		--pg-user $(PG_USER) \
		--pg-password $(PG_PASSWORD) \
		--pg-database $(PG_DATABASE) \
		--s3-bucket $(S3_BUCKET) \
		--aws-access-key-id $(S3_ACCESS_KEY) \
		--aws-secret-access-key $(S3_SECRET_KEY) \
		--aws-endpoint-url $(S3_ENDPOINT) \
		--passphrase $(PASSPHRASE) \
		--compression-enabled \
		--compression-level 10

backup-list:
	cargo run -- backup list \
		--s3-bucket $(S3_BUCKET) \
		--aws-access-key-id $(S3_ACCESS_KEY) \
		--aws-secret-access-key $(S3_SECRET_KEY) \
		--aws-endpoint-url $(S3_ENDPOINT)

backup-restore:
	cargo run -- backup restore \
		--pg-host $(PG_HOST) \
		--pg-user $(PG_USER) \
		--pg-password $(PG_PASSWORD) \
		--pg-database $(PG_DATABASE) \
		--s3-bucket $(S3_BUCKET) \
		--aws-access-key-id $(S3_ACCESS_KEY) \
		--aws-secret-access-key $(S3_SECRET_KEY) \
		--aws-endpoint-url $(S3_ENDPOINT) \
		--passphrase $(PASSPHRASE)

database-list:
	cargo run -- database list \
		--pg-host $(PG_HOST) \
		--pg-user $(PG_USER) \
		--pg-password $(PG_PASSWORD) \
		--pg-database $(PG_DATABASE)

backup-and-restore: backup
	@echo "Getting most recent backup..."
	@LATEST_BACKUP=$$(cargo run -- backup list \
		--s3-bucket $(S3_BUCKET) \
		--aws-access-key-id $(S3_ACCESS_KEY) \
		--aws-secret-access-key $(S3_SECRET_KEY) \
		--aws-endpoint-url $(S3_ENDPOINT) | grep "$(PG_DATABASE)" | grep -v "+-" | grep -v "| Filename" | head -n1 | sed 's/|//g' | awk '{print $$1}') && \
	if [ -z "$$LATEST_BACKUP" ]; then \
		echo "Error: No backup found for database $(PG_DATABASE)" >&2; \
		exit 1; \
	fi && \
	echo "Restoring from backup: $$LATEST_BACKUP to database: $(RESTORE_DATABASE)" && \
	cargo run -- backup restore \
		--pg-host $(PG_HOST) \
		--pg-user $(PG_USER) \
		--pg-password $(PG_PASSWORD) \
		--pg-database $(RESTORE_DATABASE) \
		--s3-bucket $(S3_BUCKET) \
		--aws-access-key-id $(S3_ACCESS_KEY) \
		--aws-secret-access-key $(S3_SECRET_KEY) \
		--aws-endpoint-url $(S3_ENDPOINT) \
		--passphrase $(PASSPHRASE) \
		--s3-key "$$LATEST_BACKUP"

restore-from:
	@if [ -z "$(BACKUP_FILE)" ]; then \
		echo "Error: BACKUP_FILE is required. Usage: make restore-from BACKUP_FILE=filename.sql.zst.age" >&2; \
		exit 1; \
	fi && \
	echo "Restoring from backup: $(BACKUP_FILE) to database: $(RESTORE_DATABASE)" && \
	cargo run -- backup restore \
		--pg-host $(PG_HOST) \
		--pg-user $(PG_USER) \
		--pg-password $(PG_PASSWORD) \
		--pg-database $(RESTORE_DATABASE) \
		--s3-bucket $(S3_BUCKET) \
		--aws-access-key-id $(S3_ACCESS_KEY) \
		--aws-secret-access-key $(S3_SECRET_KEY) \
		--aws-endpoint-url $(S3_ENDPOINT) \
		--passphrase $(PASSPHRASE) \
		--s3-key "$(BACKUP_FILE)"

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