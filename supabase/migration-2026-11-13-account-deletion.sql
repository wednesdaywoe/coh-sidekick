-- Migration, 2026-11-13 — a deleted account takes its published builds with it.
--
-- Lifted verbatim from the end of `schema.sql`. Run it whole in the SQL editor;
-- safe to re-run. Changes one foreign key and nothing else.
--
-- shared_builds.user_id had no ON DELETE rule, so an account that owned any
-- build could not be deleted at all, from the dashboard or the delete-account
-- function. Its builds now go with it, and favorites.build_id already cascades
-- from shared_builds, so other people's favourites of those builds go too.
-- Preview images live in Storage, which no cascade reaches; delete-account
-- removes them first.

ALTER TABLE shared_builds DROP CONSTRAINT IF EXISTS shared_builds_user_id_fkey;
ALTER TABLE shared_builds ADD CONSTRAINT shared_builds_user_id_fkey
  FOREIGN KEY (user_id) REFERENCES auth.users(id) ON DELETE CASCADE;
