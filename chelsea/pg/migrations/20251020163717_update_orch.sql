-- migrate:up

ALTER TABLE orchestrators
ALTER COLUMN created_at SET DEFAULT now();


-- migrate:down
