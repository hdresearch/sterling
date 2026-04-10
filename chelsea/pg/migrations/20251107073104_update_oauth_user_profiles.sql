-- migrate:up

ALTER TABLE oauth_user_profiles ADD avatar_uri CITEXT;

-- migrate:down


