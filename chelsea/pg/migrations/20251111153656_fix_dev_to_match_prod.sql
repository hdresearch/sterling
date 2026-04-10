-- migrate:up

ALTER TABLE "nodes" ADD COLUMN "updated_at" timestamp with time zone DEFAULT now() NOT NULL;

ALTER TABLE "orchestrators" ALTER COLUMN "created_at" DROP DEFAULT;

CREATE TABLE "password_reset_tokens" (
       "user_id" uuid NOT NULL,
       "created_at" timestamp with time zone DEFAULT now() NOT NULL,
       "expires_at" timestamp with time zone DEFAULT (now() + '00:30:00'::interval) NOT NULL,
       "used_at" timestamp with time zone,
       "token_hash" bytea NOT NULL,
       "requested_ip" inet,
       "requested_ua" text COLLATE "pg_catalog"."default"
);

CREATE UNIQUE INDEX password_reset_tokens_pkey ON public.password_reset_tokens USING btree (token_hash);

ALTER TABLE "password_reset_tokens" ADD CONSTRAINT "password_reset_tokens_pkey" PRIMARY KEY USING INDEX "password_reset_tokens_pkey";

ALTER TABLE "password_reset_tokens" ADD CONSTRAINT "password_reset_tokens_user_id_fkey" FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE NOT VALID;

ALTER TABLE "password_reset_tokens" VALIDATE CONSTRAINT "password_reset_tokens_user_id_fkey";

-- migrate:down

