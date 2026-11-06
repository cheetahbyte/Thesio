package vault

import "time"

// Day is a day in seconds.
const Day = 86_400

// Now is wall-clock Unix seconds.
func Now() int64 { return time.Now().Unix() }

// NowMillis is wall-clock Unix milliseconds.
func NowMillis() int64 { return time.Now().UnixMilli() }

// Civil is local civil time from a Unix timestamp.
func Civil(secs int64) (y int, m, d, hh, mm int) {
	t := time.Unix(secs, 0).Local()
	return t.Year(), int(t.Month()), t.Day(), t.Hour(), t.Minute()
}
