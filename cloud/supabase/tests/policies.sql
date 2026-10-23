\set ON_ERROR_STOP 0
\c ecad
-- 1. Anonymous: can read, cannot insert.
set role anon;
select count(*) as anon_reads from public.manufacturer_parts;
insert into public.manufacturer_parts (part_number, manufacturer) values ('X', 'Y');
reset role;
-- 2. Elliott inserts, claiming to be someone else: created_by is forced to him.
set role authenticated; set request.jwt.claim.sub = '11111111-1111-1111-1111-111111111111';
insert into public.manufacturer_parts (part_number, manufacturer, component_type, pin_map, created_by, status, report_count)
  values ('R-24DC', 'Acme', 'Relay', '{"A1": {"x": 0, "y": 5}}', '22222222-2222-2222-2222-222222222222', 'hidden', 99);
select part_number, github_login, created_by = auth.uid() as mine, status, report_count from public.manufacturer_parts;
-- 3. Owner edits content but not credit/moderation.
update public.manufacturer_parts set description = 'Pump relay', created_by = null, status = 'hidden' where part_number = 'R-24DC';
select description, created_by is not null as still_credited, status from public.manufacturer_parts;
-- 4. Duplicate part number for the same manufacturer is refused; bad JSON shape refused; licence fixed.
insert into public.manufacturer_parts (part_number, manufacturer) values ('R-24DC', 'Acme');
insert into public.manufacturer_parts (part_number, manufacturer, pin_map) values ('Z1', 'Acme', '[1,2]');
insert into public.manufacturer_parts (part_number, manufacturer, license) values ('Z2', 'Acme', 'proprietary');
-- 5. Someone else cannot edit or delete it.
set request.jwt.claim.sub = '22222222-2222-2222-2222-222222222222';
update public.manufacturer_parts set description = 'vandal' where part_number = 'R-24DC';
delete from public.manufacturer_parts where part_number = 'R-24DC';
select description as after_other_user from public.manufacturer_parts where part_number = 'R-24DC';
-- 6. Rate limit: the 21st upload within an hour is refused.
do $$ begin for i in 1..19 loop insert into public.manufacturer_parts (part_number, manufacturer) values ('B' || i, 'Bulk'); end loop; end $$;
insert into public.manufacturer_parts (part_number, manufacturer) values ('B20', 'Bulk');
insert into public.manufacturer_parts (part_number, manufacturer) values ('B21', 'Bulk');
reset role;
select count(*) as other_users_parts from public.manufacturer_parts where github_login = 'other';
-- 7. Reports: five reports hide a part (reporters must be real users; add three more).
insert into auth.users select gen_random_uuid(), '{}' from generate_series(1, 4);
do $$ declare u uuid; begin
  for u in select id from auth.users loop
    perform set_config('request.jwt.claim.sub', u::text, false);
    execute 'set role authenticated';
    insert into public.part_reports (part_id, reason) select id, 'wrong pins' from public.manufacturer_parts where part_number = 'R-24DC';
    execute 'reset role';
  end loop; end $$;
select status, report_count from public.manufacturer_parts where part_number = 'R-24DC';
set role anon;
select count(*) as anon_sees_hidden_part from public.manufacturer_parts where part_number = 'R-24DC';
