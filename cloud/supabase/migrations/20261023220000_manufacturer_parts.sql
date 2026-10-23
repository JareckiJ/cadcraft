-- CADCraft ECAD community parts database (Supabase / PostgreSQL).
--
-- Anyone can read published parts. Signed-in users (GitHub login through Supabase Auth) can add
-- parts, which are credited to them (created_by is always their own auth.uid(), whatever the
-- client sends), and can edit or delete only their own. Uploads are rate-limited per user, parts
-- are shared under CC BY 4.0, and parts reported by several users are hidden for review.
--
-- Apply with the Supabase CLI (`supabase db push`) or paste into the SQL editor.

create extension if not exists pg_trgm with schema extensions;

-- ---------------------------------------------------------------------------------------------
-- Parts
-- ---------------------------------------------------------------------------------------------

create table if not exists public.manufacturer_parts (
    id              uuid primary key default gen_random_uuid(),
    part_number     text not null check (char_length(part_number) between 1 and 128),
    manufacturer    text not null check (char_length(manufacturer) between 1 and 128),
    -- Relay, Contactor, Breaker, VFD, Fuse, Motor, Terminal, PSU, PLC, Sensor, Lamp, Other…
    component_type  text not null default 'Other' check (char_length(component_type) between 1 and 64),
    -- IEC 81346-2 class letters of the symbol (K, Q, F, M…).
    iec_class       text check (iec_class ~ '^[A-Z]{1,3}$'),
    description     text not null default '' check (char_length(description) <= 512),
    -- {"A1": {"x": 0, "y": 5, "dir": "up"}, "A2": {…}}: pin names to symbol coordinates.
    pin_map         jsonb not null default '{}'::jsonb check (jsonb_typeof(pin_map) = 'object'),
    -- Ratings and other data: {"coilVoltage": {"value": 24, "unit": "V DC"}, "footprint": "…"}.
    attributes      jsonb not null default '{}'::jsonb check (jsonb_typeof(attributes) = 'object'),
    -- The symbol drawing: {"lines": [[x1,y1,x2,y2]…], "circles": [[x,y,r]…], "arcs": […]}.
    cad_geometry    jsonb not null default '{}'::jsonb check (jsonb_typeof(cad_geometry) = 'object'),
    license         text not null default 'CC-BY-4.0' check (license = 'CC-BY-4.0'),
    created_by      uuid references auth.users (id) on delete set null default auth.uid(),
    -- The contributor's GitHub login, for credit (filled from their auth profile).
    github_login    text,
    status          text not null default 'published' check (status in ('published', 'hidden')),
    report_count    integer not null default 0,
    created_at      timestamptz not null default now(),
    updated_at      timestamptz not null default now(),
    -- One record per part of a manufacturer.
    unique (manufacturer, part_number),
    -- Keep records small (a symbol is a few kB).
    check (octet_length(pin_map::text) + octet_length(attributes::text) + octet_length(cad_geometry::text) < 1048576)
);

create index if not exists manufacturer_parts_search
    on public.manufacturer_parts using gin ((part_number || ' ' || manufacturer || ' ' || component_type) extensions.gin_trgm_ops);
create index if not exists manufacturer_parts_created_by on public.manufacturer_parts (created_by);

-- Credit, ownership and the rate limit, enforced on the server.
create or replace function public.manufacturer_parts_before_write()
returns trigger
language plpgsql
security definer
set search_path = public
as $$
declare
    recent integer;
begin
    if tg_op = 'INSERT' then
        new.created_by := auth.uid();
        new.github_login := coalesce(
            (select raw_user_meta_data ->> 'user_name' from auth.users where id = auth.uid()),
            (select raw_user_meta_data ->> 'preferred_username' from auth.users where id = auth.uid()));
        new.status := 'published';
        new.report_count := 0;
        new.created_at := now();
        select count(*) into recent
          from public.manufacturer_parts
         where created_by = auth.uid() and created_at > now() - interval '1 hour';
        if recent >= 20 then
            raise exception 'Upload limit reached: at most 20 parts an hour. Please try again later.'
                using errcode = 'P0001';
        end if;
    elsif coalesce(current_setting('cadcraft.moderation', true), '') <> 'on' then
        -- Owners edit content; credit, moderation and timestamps stay the server's.
        new.id := old.id;
        new.created_by := old.created_by;
        new.github_login := old.github_login;
        new.status := old.status;
        new.report_count := old.report_count;
        new.created_at := old.created_at;
    end if;
    new.updated_at := now();
    return new;
end;
$$;

drop trigger if exists manufacturer_parts_before_write on public.manufacturer_parts;
create trigger manufacturer_parts_before_write
    before insert or update on public.manufacturer_parts
    for each row execute function public.manufacturer_parts_before_write();

alter table public.manufacturer_parts enable row level security;

-- Anyone (signed in or not) reads published parts; contributors also see their own hidden ones.
drop policy if exists "read published parts" on public.manufacturer_parts;
create policy "read published parts" on public.manufacturer_parts
    for select to anon, authenticated
    using (status = 'published' or created_by = auth.uid());

-- Signed-in users add parts as themselves.
drop policy if exists "add own parts" on public.manufacturer_parts;
create policy "add own parts" on public.manufacturer_parts
    for insert to authenticated
    with check (created_by = auth.uid());

-- And edit or delete only their own.
drop policy if exists "edit own parts" on public.manufacturer_parts;
create policy "edit own parts" on public.manufacturer_parts
    for update to authenticated
    using (created_by = auth.uid())
    with check (created_by = auth.uid());

drop policy if exists "delete own parts" on public.manufacturer_parts;
create policy "delete own parts" on public.manufacturer_parts
    for delete to authenticated
    using (created_by = auth.uid());

-- ---------------------------------------------------------------------------------------------
-- Reports: five reports from different users hide a part until a maintainer looks at it.
-- ---------------------------------------------------------------------------------------------

create table if not exists public.part_reports (
    part_id     uuid not null references public.manufacturer_parts (id) on delete cascade,
    reporter    uuid not null references auth.users (id) on delete cascade default auth.uid(),
    reason      text not null check (char_length(reason) between 1 and 500),
    created_at  timestamptz not null default now(),
    primary key (part_id, reporter)
);

alter table public.part_reports enable row level security;

drop policy if exists "report as yourself" on public.part_reports;
create policy "report as yourself" on public.part_reports
    for insert to authenticated
    with check (reporter = auth.uid());

drop policy if exists "see own reports" on public.part_reports;
create policy "see own reports" on public.part_reports
    for select to authenticated
    using (reporter = auth.uid());

create or replace function public.part_reports_after_insert()
returns trigger
language plpgsql
security definer
set search_path = public
as $$
begin
    -- Lets the update below change the moderation columns (owners cannot).
    perform set_config('cadcraft.moderation', 'on', true);
    update public.manufacturer_parts
       set report_count = (select count(*) from public.part_reports where part_id = new.part_id),
           status = case when (select count(*) from public.part_reports where part_id = new.part_id) >= 5
                         then 'hidden' else status end
     where id = new.part_id;
    perform set_config('cadcraft.moderation', 'off', true);
    return new;
end;
$$;

drop trigger if exists part_reports_after_insert on public.part_reports;
create trigger part_reports_after_insert
    after insert on public.part_reports
    for each row execute function public.part_reports_after_insert();

-- The client (PostgREST) roles.
grant select on public.manufacturer_parts to anon, authenticated;
grant insert, update, delete on public.manufacturer_parts to authenticated;
grant select, insert on public.part_reports to authenticated;
