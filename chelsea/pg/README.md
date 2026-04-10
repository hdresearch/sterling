# Readme

## Background

**We want the schema stored in the code to match what is in production.**

This _seems_ simple enough. We write a simple bash script, that runs in CI that
migrates the DB whenever the mainline branch is pushed.

This encounters at least four problems:

1. In what order should the migrations be run?

2. Which migrations have already been applied and which are pending?

3. How do we avoid locking the production DB (and thereby locking up the entire
   system)?

4. How to handle conflicts between what the currently running code expects, and
   what the migrated DB provides?

I think most people (and tools) are aware of the first two problems. They are
generally addressed by a combination of migration file naming conventions, and a
table in the DB that stores which migrations have been applied.

This breaks some of the time because the most correct behavior is to run the
migrations in the order that they are merged to main, and to only run the
migrations that are included in the merge. This can be different from the order
in which the migrations where created (and named), so naming conflicts or miss
ordering of migrations is common.

The third problem is more subtle: some changes will require the database to do a
substantial amount of work, while it is doing this work, the table(s) involved
may be locked, which can bring the rest of the system to a halt.

In general any SQL statement that causes a table rewrite, an index rewrite, or
that is used to modify a large amount of existing data can lead to this problem.

An example of an apparently innocuous, but potentially problematic migration

```sql
ALTER TABLE api_keys
ALTER COLUMN created_at set DEFAULT now();
```

Will cause the `api_keys` table to be re-written. If the table is large, this
could take some time (I've seen migrations that take hours to run).

Even if the migration itself can run quickly, lock contention can extend that
time:

1. The migration needs an exclusive lock on a table.

2. Other, potentially long lived process(s) have conflicting locks on the table.

3. All new processes that require conflicting locks have to either wait for the
   migration to get its lock and complete its run -or- the migration has to keep
   trying until no other locks are pending.

Depending on the approach used, the system either appears down as the new
processes lock's queue up, or the migration never gets its lock and never runs.

Additionally all migrations that involve a significant amount of work for the DB
(table & index rewrites generally) will consume (potentially substantial) disk,
CPU and RAM resources. Large tables may be completely duplicated during the
migration.

In the past I've typically addressed all three problems by human intervention —
by appointing a release manager who was the only person allowed to make
releases, and who is responsible for all database migrations.

They manually run the migrations, while watching the monitoring system, and if
required, pause or back out the changes. They also frequently had to re-write
the migrations from the clear, simple statements in the migration files, to
multi-step processes to minimize impact on the DB.

The only solution I am aware of to the \#4th problem is to modify how code is
written and merged into main to insure backward compatibility.

For example, if you need to re-name a column, the steps are become:

1. Write a migration that adds the new column name, that copies all existing
   data from old column name to new column name, and that installs a trigger to
   keep them in sync.

2. Change the code to use the new column name & release it.

3. Write a migration which drops the old column name, and associated triggers.


## The Solution

Part of this change is tooling, but it involves more than that — it also a
change in how we write code and make releases.

1. Be aware that merges to main are going to make a release and database
   migration.

2. Main should always move forward. A `git revert` or `force push` to point the
   branch name at another commit will generally cause chaos.

3. Code must be written following the backward compatibility steps above.

4. We will use the tool [Dbmate](https://github.com/amacneil/dbmate) to manage
   migrations.

5. You are encouraged to use [pg-schema-diff](https://github.com/stripe/pg-schema-diff)
   to create / review migrations. It appears to do a somewhat good job of at
   least warning you which migrations have hazards.
