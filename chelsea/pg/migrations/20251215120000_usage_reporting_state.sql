-- migrate:up

CREATE TABLE IF NOT EXISTS usage_reporting_state (
    orchestrator_id UUID PRIMARY KEY,
    last_interval_start BIGINT NOT NULL,
    last_interval_end BIGINT NOT NULL,
    last_report_time TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- migrate:down

DROP TABLE IF EXISTS usage_reporting_state;
