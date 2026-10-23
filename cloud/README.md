# ECAD community parts database (Supabase)

CADCraft's Electrical tools can share manufacturer parts (symbol drawing, pin map, ratings) through
a Supabase project: anyone can search and download them, people signed in with GitHub can share
their own.

## Setting up the Supabase project (once)

1. Create a project at supabase.com. Note its **Project URL** and **anon public key**
   (Settings → API). These two are public and go into CADCraft. The **service_role key must never
   be put in CADCraft or shared**.
2. Create the tables: SQL Editor → paste `supabase/migrations/20261023220000_manufacturer_parts.sql`
   → Run (or `supabase db push` with the CLI).
3. GitHub login:
   - On GitHub: Settings → Developer settings → OAuth Apps → New OAuth App.
     Homepage URL: your project URL. Authorization callback URL:
     `https://<project-ref>.supabase.co/auth/v1/callback`. Copy the Client ID and generate a
     Client secret.
   - In Supabase: Authentication → Sign In / Providers → GitHub → enable, paste the Client ID and
     secret (the secret stays in Supabase).
   - Authentication → URL Configuration → Redirect URLs → add
     `http://127.0.0.1:53682/callback` (where CADCraft listens while you sign in).
4. In CADCraft: **Tools → Electrical → Database → Community Hub**, paste the Project URL and anon
   key, then **Link GitHub Account**.

## What the database enforces

- Everyone reads published parts; only signed-in users add parts, credited to themselves
  (`created_by` is always their own user id, with their GitHub login for credit).
- Contributors edit and delete only their own parts; credit and moderation fields are the
  server's.
- At most 20 uploads per user per hour.
- Parts are shared under **CC BY 4.0**. Share only parts you drew yourself: manufacturer or
  EPLAN-derived drawings are usually not yours to redistribute.
- Five reports from different users hide a part until a maintainer reviews it.

`supabase/tests/` runs these rules on a plain PostgreSQL with a small stand-in for Supabase's
`auth` schema: `psql -f tests/mock_supabase.sql`, then the migration on database `ecad`, then
`psql -f tests/policies.sql`.
