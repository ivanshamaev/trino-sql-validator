create table hive.ride_sharing_dataset.trips_data_bing with (format = 'PARQUET') as
select
	bing_tile_quadkey(bing_tile_at(lat,
	lon,
	19)) as qkey,
	*
from
	hive.ride_sharing_dataset.trips_data;


select
	count(distinct rider_id)
from
	hive.ride_sharing_dataset.trips_data as t,
	hive.ride_sharing_dataset.places as p
where
	great_circle_distance(t.lat,
	t.lon,
	p.lat,
	p.lon) <= 0.5;


select
	count(distinct driver_id)
from
	ride_sharing_dataset.trips_data t
where
	ST_Contains(ST_Polygon('POLYGON ((-122.45635986328125 37.72130604487681, -122.45567321777344 37.72130604487681, -122.45567321777344 37.72184917678752, -122.45635986328125 37.72184917678752, -122.45635986328125 37.072130604487681))'),
	st_point(lon,
	lat));


select
	count(distinct driver_id)
from
	ride_sharing_dataset.trips_data t
where
	lon between -122.45635986328125 and -122.45567321777344
	and lat between 37.72130604487681 and 37.72184917678752
	and ST_Contains(ST_Polygon('POLYGON ((-122.45635986328125 37.72130604487681, -122.45567321777344 37.72130604487681, -122.45567321777344 37.72184917678752, -122.45635986328125 37.72184917678752))'),
	st_point(lon,
	lat));


with p as (
select
	lat,
	lon,
	qkey
from
	ride_sharing_dataset.places
cross join unnest (transform(bing_tiles_around(lat,
	lon,
	19,
	0.5),
	t -> bing_tile_quadkey(t))) as t(qkey))
-- use that list to filter matching tiles
 select
	count(distinct rider_id)
from
	ride_sharing_dataset.trips_data_bing as v,
	r
where
	t.qkey = p.qkey
	-- match the bounding tile
	and great_circle_distance(t.lat,
	t.lon,
	p.lat,
	p.lon) <= 0.5;


with q as (
select
	qkey
from
	(
values ('POLYGON ((-122.45635986328125 37.72130604487681, -122.45567321777344 37.72130604487681, -122.45567321777344 37.72184917678752, -122.45635986328125 37.72184917678752)) ') ) as a(p)
cross join unnest (transform(geometry_to_bing_tiles(ST_Polygon(a.p),
	19),
	t -> bing_tile_quadkey(t))) as t(qkey) )
select
	count(distinct driver_id)
from
	ride_sharing_dataset.trips_data_bing v,
	q
where
	q.qkey = t.qkey
	and ST_Contains(ST_Polygon('POLYGON ((-122.45635986328125 37.72130604487681, -122.45567321777344 37.72130604487681, -122.45567321777344 37.72184917678752, -122.45635986328125 37.72184917678752))'),
	st_point(lon,
	lat));
