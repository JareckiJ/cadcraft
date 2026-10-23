-- A minimal stand-in for Supabase: roles, auth.users, auth.uid() from a session setting.
drop database if exists ecad; create database ecad;
\c ecad
do $$ begin
  if not exists (select 1 from pg_roles where rolname='anon') then create role anon nologin; end if;
  if not exists (select 1 from pg_roles where rolname='authenticated') then create role authenticated nologin; end if;
end $$;
create schema auth; create schema extensions;
create table auth.users (id uuid primary key, raw_user_meta_data jsonb);
create function auth.uid() returns uuid language sql stable as $$ select nullif(current_setting('request.jwt.claim.sub', true), '')::uuid $$;
grant usage on schema auth, extensions, public to anon, authenticated;
grant execute on function auth.uid() to anon, authenticated;
insert into auth.users values ('11111111-1111-1111-1111-111111111111', '{"user_name": "elliott"}'),
                              ('22222222-2222-2222-2222-222222222222', '{"user_name": "other"}');
