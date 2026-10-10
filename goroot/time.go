// Copyright 2009 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style
// license that can be found in the LICENSE file.

// Package time measures and displays time.
//
// This is not a port of the standard library's implementation, which sits on
// the runtime's timers, the monotonic clock and the zoneinfo database. What is
// provided is the deterministic subset: Duration arithmetic and printing, Time
// values in UTC or a fixed offset (Date, Unix, Add, Sub, Format, Parse, the
// calendar accessors), and Sleep, After, Tick, Timer and Ticker on the
// cooperative scheduler.
//
// fusevm's goroutines are green threads that switch only at channel
// operations, and the scheduler has no timer queue. A sleeper therefore waits
// by re-checking the clock and yielding (`yield`) until its deadline passes;
// the registry below makes the sleepers wake in deadline order regardless of
// how the round-robin visits them, which is the order Go's timer heap gives.
// `Local` is UTC: the machine's zone is not part of a deterministic program.
package time

// A Duration represents the elapsed time between two instants as an int64
// nanosecond count.
type Duration int64

const (
	Nanosecond  Duration = 1
	Microsecond          = 1000 * Nanosecond
	Millisecond          = 1000 * Microsecond
	Second               = 1000 * Millisecond
	Minute               = 60 * Second
	Hour                 = 60 * Minute
)

const (
	minDuration Duration = -1 << 63
	maxDuration Duration = 1<<63 - 1
)

// decimal renders u / 10^prec, dropping trailing zeros of the fraction.
func decimal(u uint64, prec int) string {
	div := uint64(1)
	for i := 0; i < prec; i++ {
		div *= 10
	}
	whole := u / div
	frac := u % div
	s := utoa(whole)
	if frac == 0 {
		return s
	}
	digits := utoa(frac)
	for len(digits) < prec {
		digits = "0" + digits
	}
	end := len(digits)
	for end > 0 && digits[end-1] == '0' {
		end--
	}
	return s + "." + digits[:end]
}

func utoa(u uint64) string {
	if u == 0 {
		return "0"
	}
	s := ""
	for u > 0 {
		s = string(rune('0'+u%10)) + s
		u /= 10
	}
	return s
}

// String returns a string representing the duration in the form "72h3m0.5s".
// Leading zero units are omitted. As a special case, durations less than one
// second format use a smaller unit (milli-, micro-, or nanoseconds) to ensure
// that the leading digit is non-zero. The zero duration formats as 0s.
func (d Duration) String() string {
	if d == 0 {
		return "0s"
	}
	u := uint64(d)
	neg := d < 0
	if neg {
		u = -u
	}
	s := ""
	if u < uint64(Second) {
		switch {
		case u < uint64(Microsecond):
			s = utoa(u) + "ns"
		case u < uint64(Millisecond):
			s = decimal(u, 3) + "µs"
		default:
			s = decimal(u, 6) + "ms"
		}
	} else {
		secs := decimal(u%(60*uint64(Second)), 9) + "s"
		u /= 60 * uint64(Second)
		// u is now integer minutes
		s = secs
		if u > 0 {
			s = utoa(u%60) + "m" + s
			u /= 60
			if u > 0 {
				s = utoa(u) + "h" + s
			}
		}
	}
	if neg {
		s = "-" + s
	}
	return s
}

// Nanoseconds returns the duration as an integer nanosecond count.
func (d Duration) Nanoseconds() int64 { return int64(d) }

// Microseconds returns the duration as an integer microsecond count.
func (d Duration) Microseconds() int64 { return int64(d) / 1000 }

// Milliseconds returns the duration as an integer millisecond count.
func (d Duration) Milliseconds() int64 { return int64(d) / 1000000 }

// Seconds returns the duration as a floating point number of seconds.
func (d Duration) Seconds() float64 {
	sec := d / Second
	nsec := d % Second
	return float64(sec) + float64(nsec)/1000000000
}

// Minutes returns the duration as a floating point number of minutes.
func (d Duration) Minutes() float64 {
	min := d / Minute
	nsec := d % Minute
	return float64(min) + float64(nsec)/(60*1000000000)
}

// Hours returns the duration as a floating point number of hours.
func (d Duration) Hours() float64 {
	h := d / Hour
	nsec := d % Hour
	return float64(h) + float64(nsec)/(60*60*1000000000)
}

// Truncate returns the result of rounding d toward zero to a multiple of m.
// If m <= 0, Truncate returns d unchanged.
func (d Duration) Truncate(m Duration) Duration {
	if m <= 0 {
		return d
	}
	return d - d%m
}

// Round returns the result of rounding d to the nearest multiple of m.
// The rounding behavior for halfway values is to round away from zero.
// If m <= 0, Round returns d unchanged.
func (d Duration) Round(m Duration) Duration {
	if m <= 0 {
		return d
	}
	r := d % m
	if d < 0 {
		r = -r
		if r+r < m {
			return d + r
		}
		return d - m + r
	}
	if r+r < m {
		return d - r
	}
	return d + m - r
}

// Abs returns the absolute value of d.
func (d Duration) Abs() Duration {
	switch {
	case d >= 0:
		return d
	case d == minDuration:
		return maxDuration
	default:
		return -d
	}
}

// ParseDuration parses a duration string such as "300ms", "-1.5h" or "2h45m".
// Valid time units are "ns", "us" (or "µs"), "ms", "s", "m", "h".
func ParseDuration(s string) (Duration, error) {
	orig := s
	var d uint64
	neg := false
	if s != "" {
		c := s[0]
		if c == '-' || c == '+' {
			neg = c == '-'
			s = s[1:]
		}
	}
	if s == "0" {
		return 0, nil
	}
	if s == "" {
		return 0, &durationError{"time: invalid duration " + quote(orig)}
	}
	for s != "" {
		var v, f uint64
		scale := 1.0
		var err bool
		if !(s[0] == '.' || '0' <= s[0] && s[0] <= '9') {
			return 0, &durationError{"time: invalid duration " + quote(orig)}
		}
		pl := len(s)
		v, s, err = leadingInt(s)
		if err {
			return 0, &durationError{"time: invalid duration " + quote(orig)}
		}
		pre := pl != len(s)
		post := false
		if s != "" && s[0] == '.' {
			s = s[1:]
			pl := len(s)
			f, scale, s = leadingFraction(s)
			post = pl != len(s)
		}
		if !pre && !post {
			return 0, &durationError{"time: invalid duration " + quote(orig)}
		}
		i := 0
		for ; i < len(s); i++ {
			c := s[i]
			if c == '.' || '0' <= c && c <= '9' {
				break
			}
		}
		if i == 0 {
			return 0, &durationError{"time: missing unit in duration " + quote(orig)}
		}
		u := s[:i]
		s = s[i:]
		var unit uint64
		switch u {
		case "ns":
			unit = uint64(Nanosecond)
		case "us", "µs", "μs":
			unit = uint64(Microsecond)
		case "ms":
			unit = uint64(Millisecond)
		case "s":
			unit = uint64(Second)
		case "m":
			unit = uint64(Minute)
		case "h":
			unit = uint64(Hour)
		default:
			return 0, &durationError{"time: unknown unit " + quote(u) + " in duration " + quote(orig)}
		}
		if v > 1<<63/unit {
			return 0, &durationError{"time: invalid duration " + quote(orig)}
		}
		v *= unit
		if f > 0 {
			v += uint64(float64(f) * (float64(unit) / scale))
			if v > 1<<63 {
				return 0, &durationError{"time: invalid duration " + quote(orig)}
			}
		}
		d += v
		if d > 1<<63 {
			return 0, &durationError{"time: invalid duration " + quote(orig)}
		}
	}
	if neg {
		return -Duration(d), nil
	}
	if d > 1<<63-1 {
		return 0, &durationError{"time: invalid duration " + quote(orig)}
	}
	return Duration(d), nil
}

type durationError struct{ msg string }

func (e *durationError) Error() string { return e.msg }

func quote(s string) string { return "\"" + s + "\"" }

func leadingInt(s string) (x uint64, rem string, err bool) {
	i := 0
	for ; i < len(s); i++ {
		c := s[i]
		if c < '0' || c > '9' {
			break
		}
		if x > 1<<63/10 {
			return 0, rem, true
		}
		x = x*10 + uint64(c) - '0'
		if x > 1<<63 {
			return 0, rem, true
		}
	}
	return x, s[i:], false
}

func leadingFraction(s string) (x uint64, scale float64, rem string) {
	i := 0
	scale = 1
	overflow := false
	for ; i < len(s); i++ {
		c := s[i]
		if c < '0' || c > '9' {
			break
		}
		if overflow {
			continue
		}
		if x > (1<<63-1)/10 {
			overflow = true
			continue
		}
		y := x*10 + uint64(c) - '0'
		if y > 1<<63 {
			overflow = true
			continue
		}
		x = y
		scale *= 10
	}
	return x, scale, s[i:]
}

// A Month specifies a month of the year (January = 1, ...).
type Month int

const (
	January Month = 1 + iota
	February
	March
	April
	May
	June
	July
	August
	September
	October
	November
	December
)

var longMonthNames = []string{"January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"}

var shortMonthNames = []string{"Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"}

// String returns the English name of the month ("January", "February", ...).
func (m Month) String() string {
	if January <= m && m <= December {
		return longMonthNames[m-1]
	}
	return "%!Month(" + utoa(uint64(m)) + ")"
}

// A Weekday specifies a day of the week (Sunday = 0, ...).
type Weekday int

const (
	Sunday Weekday = iota
	Monday
	Tuesday
	Wednesday
	Thursday
	Friday
	Saturday
)

var longDayNames = []string{"Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"}

var shortDayNames = []string{"Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"}

// String returns the English name of the day ("Sunday", "Monday", ...).
func (d Weekday) String() string {
	if Sunday <= d && d <= Saturday {
		return longDayNames[d]
	}
	return "%!Weekday(" + utoa(uint64(d)) + ")"
}

// A Location maps time instants to the zone in use at that time. Only UTC and
// fixed offsets exist here.
type Location struct {
	name   string
	offset int
}

var utcLoc = Location{"UTC", 0}

// UTC represents Universal Coordinated Time.
var UTC *Location = &utcLoc

// Local represents the system's local time zone, which is UTC here.
var Local *Location = &Location{"Local", 0}

// FixedZone returns a Location that always uses the given zone name and offset
// (seconds east of UTC).
func FixedZone(name string, offset int) *Location {
	return &Location{name, offset}
}

// String returns a descriptive name for the time zone information.
func (l *Location) String() string {
	if l == nil {
		return "UTC"
	}
	return l.name
}

// A Time represents an instant in time with nanosecond precision.
//
// sec counts seconds since January 1, year 1 00:00:00 UTC, so the zero Time is
// that instant, as in Go. mono is the monotonic reading of a Now() value, or 0.
type Time struct {
	sec  int64
	nsec int64
	mono int64
	loc  *Location
}

const (
	secondsPerDay  = 86400
	unixToInternal = (1969*365 + 1969/4 - 1969/100 + 1969/400) * secondsPerDay
)

// nowNano, monoNano, yield and sleepHint are host intrinsics, not declared
// here: the wall clock in Unix nanoseconds, the monotonic clock, a scheduler
// yield point, and a bounded real sleep that keeps a waiting goroutine from
// burning a core.

// Now returns the current local time.
func Now() Time {
	n := nowNano()
	return Time{n/1000000000 + unixToInternal, n % 1000000000, monoNano() + 1, Local}
}

// Unix returns the local Time corresponding to the given Unix time, sec seconds
// and nsec nanoseconds since January 1, 1970 UTC.
func Unix(sec int64, nsec int64) Time {
	if nsec < 0 || nsec >= 1000000000 {
		n := nsec / 1000000000
		sec += n
		nsec -= n * 1000000000
		if nsec < 0 {
			nsec += 1000000000
			sec--
		}
	}
	return Time{sec + unixToInternal, nsec, 0, Local}
}

// UnixMilli returns the local Time corresponding to the given Unix time, msec
// milliseconds since January 1, 1970 UTC.
func UnixMilli(msec int64) Time {
	return Unix(msec/1000, (msec%1000)*1000000)
}

// UnixMicro returns the local Time corresponding to the given Unix time, usec
// microseconds since January 1, 1970 UTC.
func UnixMicro(usec int64) Time {
	return Unix(usec/1000000, (usec%1000000)*1000)
}

// daysFromCivil is the number of days from 1970-01-01 to the given date.
func daysFromCivil(y int64, m int64, d int64) int64 {
	if m <= 2 {
		y--
	}
	era := y / 400
	if y < 0 {
		era = (y - 399) / 400
	}
	yoe := y - era*400
	mp := m - 3
	if m <= 2 {
		mp = m + 9
	}
	doy := (153*mp+2)/5 + d - 1
	doe := yoe*365 + yoe/4 - yoe/100 + doy
	return era*146097 + doe - 719468
}

// civilFromDays is the inverse of daysFromCivil.
func civilFromDays(z int64) (int64, int64, int64) {
	z += 719468
	era := z / 146097
	if z < 0 {
		era = (z - 146096) / 146097
	}
	doe := z - era*146097
	yoe := (doe - doe/1460 + doe/36524 - doe/146096) / 365
	y := yoe + era*400
	doy := doe - (365*yoe + yoe/4 - yoe/100)
	mp := (5*doy + 2) / 153
	d := doy - (153*mp+2)/5 + 1
	m := mp + 3
	if mp >= 10 {
		m = mp - 9
	}
	if m <= 2 {
		y++
	}
	return y, m, d
}

func floorDiv(a int64, b int64) int64 {
	q := a / b
	if a%b < 0 {
		q--
	}
	return q
}

// Date returns the Time corresponding to
//
//	yyyy-mm-dd hh:mm:ss + nsec nanoseconds
//
// in the appropriate zone for that time in the given location. The month, day,
// hour, min, sec, and nsec values may be outside their usual ranges and will be
// normalized during the conversion.
func Date(year int, month Month, day, hour, min, sec, nsec int, loc *Location) Time {
	if loc == nil {
		panic("time: missing Location in call to Date")
	}
	m := int64(month) - 1
	y := int64(year) + floorDiv(m, 12)
	m = m - floorDiv(m, 12)*12 + 1
	days := daysFromCivil(y, m, 1) + int64(day) - 1
	s := days*secondsPerDay + int64(hour)*3600 + int64(min)*60 + int64(sec)
	ns := int64(nsec)
	s += floorDiv(ns, 1000000000)
	ns -= floorDiv(ns, 1000000000) * 1000000000
	s -= int64(loc.offset)
	return Time{s + unixToInternal, ns, 0, loc}
}

func (t Time) location() *Location {
	if t.loc == nil {
		return UTC
	}
	return t.loc
}

// local is the number of seconds since 1970 in t's zone.
func (t Time) local() int64 {
	return t.sec - unixToInternal + int64(t.location().offset)
}

// IsZero reports whether t represents the zero time instant, January 1, year 1,
// 00:00:00 UTC.
func (t Time) IsZero() bool { return t.sec == 0 && t.nsec == 0 }

// After reports whether the time instant t is after u.
func (t Time) After(u Time) bool {
	return t.sec > u.sec || t.sec == u.sec && t.nsec > u.nsec
}

// Before reports whether the time instant t is before u.
func (t Time) Before(u Time) bool {
	return t.sec < u.sec || t.sec == u.sec && t.nsec < u.nsec
}

// Equal reports whether t and u represent the same time instant.
func (t Time) Equal(u Time) bool { return t.sec == u.sec && t.nsec == u.nsec }

// Compare compares the time instant t with u: -1 if before, 0 if equal, +1 if
// after.
func (t Time) Compare(u Time) int {
	switch {
	case t.Before(u):
		return -1
	case t.After(u):
		return +1
	}
	return 0
}

// Date returns the year, month, and day in which t occurs.
func (t Time) Date() (year int, month Month, day int) {
	y, m, d := civilFromDays(floorDiv(t.local(), secondsPerDay))
	return int(y), Month(m), int(d)
}

// Clock returns the hour, minute, and second within the day specified by t.
func (t Time) Clock() (hour, min, sec int) {
	s := t.local() - floorDiv(t.local(), secondsPerDay)*secondsPerDay
	return int(s / 3600), int(s % 3600 / 60), int(s % 60)
}

// Year returns the year in which t occurs.
func (t Time) Year() int {
	y, _, _ := t.Date()
	return y
}

// Month returns the month of the year specified by t.
func (t Time) Month() Month {
	_, m, _ := t.Date()
	return m
}

// Day returns the day of the month specified by t.
func (t Time) Day() int {
	_, _, d := t.Date()
	return d
}

// Hour returns the hour within the day specified by t, in the range [0, 23].
func (t Time) Hour() int {
	h, _, _ := t.Clock()
	return h
}

// Minute returns the minute offset within the hour specified by t.
func (t Time) Minute() int {
	_, m, _ := t.Clock()
	return m
}

// Second returns the second offset within the minute specified by t.
func (t Time) Second() int {
	_, _, s := t.Clock()
	return s
}

// Nanosecond returns the nanosecond offset within the second specified by t.
func (t Time) Nanosecond() int { return int(t.nsec) }

// Weekday returns the day of the week specified by t.
func (t Time) Weekday() Weekday {
	days := floorDiv(t.local(), secondsPerDay)
	w := (days + 4) % 7
	if w < 0 {
		w += 7
	}
	return Weekday(w)
}

// YearDay returns the day of the year specified by t, in the range [1,365] for
// non-leap years, and [1,366] in leap years.
func (t Time) YearDay() int {
	y, _, _ := t.Date()
	days := floorDiv(t.local(), secondsPerDay)
	return int(days-daysFromCivil(int64(y), 1, 1)) + 1
}

// Unix returns t as a Unix time, the number of seconds elapsed since January 1,
// 1970 UTC.
func (t Time) Unix() int64 { return t.sec - unixToInternal }

// UnixMilli returns t as a Unix time in milliseconds.
func (t Time) UnixMilli() int64 { return (t.sec-unixToInternal)*1000 + t.nsec/1000000 }

// UnixMicro returns t as a Unix time in microseconds.
func (t Time) UnixMicro() int64 { return (t.sec-unixToInternal)*1000000 + t.nsec/1000 }

// UnixNano returns t as a Unix time in nanoseconds.
func (t Time) UnixNano() int64 { return (t.sec-unixToInternal)*1000000000 + t.nsec }

// UTC returns t with the location set to UTC.
func (t Time) UTC() Time {
	t.loc = UTC
	return t
}

// Local returns t with the location set to local time.
func (t Time) Local() Time {
	t.loc = Local
	return t
}

// In returns a copy of t representing the same time instant, but with the
// copy's location information set to loc for display purposes.
func (t Time) In(loc *Location) Time {
	if loc == nil {
		panic("time: missing Location in call to Time.In")
	}
	t.loc = loc
	return t
}

// Location returns the time zone information associated with t.
func (t Time) Location() *Location { return t.location() }

// Zone computes the time zone in effect at time t, returning the abbreviated
// name of the zone and its offset in seconds east of UTC.
func (t Time) Zone() (name string, offset int) {
	l := t.location()
	return l.zoneName(), l.offset
}

func (l *Location) zoneName() string {
	if l.name == "Local" {
		return "UTC"
	}
	return l.name
}

// Add returns the time t+d.
func (t Time) Add(d Duration) Time {
	dsec := int64(d / 1000000000)
	nsec := t.nsec + int64(d%1000000000)
	if nsec >= 1000000000 {
		dsec++
		nsec -= 1000000000
	} else if nsec < 0 {
		dsec--
		nsec += 1000000000
	}
	t.sec += dsec
	t.nsec = nsec
	if t.mono != 0 {
		t.mono += int64(d)
	}
	return t
}

// Sub returns the duration t-u.
func (t Time) Sub(u Time) Duration {
	if t.mono != 0 && u.mono != 0 {
		return Duration(t.mono - u.mono)
	}
	return Duration((t.sec-u.sec)*1000000000 + (t.nsec - u.nsec))
}

// Since returns the time elapsed since t.
func Since(t Time) Duration { return Now().Sub(t) }

// Until returns the duration until t.
func Until(t Time) Duration { return t.Sub(Now()) }

// AddDate returns the time corresponding to adding the given number of years,
// months, and days to t.
func (t Time) AddDate(years int, months int, days int) Time {
	year, month, day := t.Date()
	hour, min, sec := t.Clock()
	return Date(year+years, month+Month(months), day+days, hour, min, sec, int(t.nsec), t.location())
}

// Truncate returns the result of rounding t down to a multiple of d (since the
// zero time). If d <= 0, Truncate returns t stripped of any monotonic clock
// reading but otherwise unchanged.
func (t Time) Truncate(d Duration) Time {
	t.mono = 0
	if d <= 0 {
		return t
	}
	r := t.sinceZero(d)
	return t.Add(-r)
}

// Round returns the result of rounding t to the nearest multiple of d (since
// the zero time). The rounding behavior for halfway values is to round up.
func (t Time) Round(d Duration) Time {
	t.mono = 0
	if d <= 0 {
		return t
	}
	r := t.sinceZero(d)
	if r+r < d {
		return t.Add(-r)
	}
	return t.Add(d - r)
}

// sinceZero is (t - zero time) mod d.
func (t Time) sinceZero(d Duration) Duration {
	if d%Second == 0 {
		s := int64(d / Second)
		return Duration(t.sec%s)*Second + Duration(t.nsec)
	}
	// t.sec * 1000000000 may exceed int64 for large t, so reduce first.
	m := int64(d)
	r := (t.sec % m) * (1000000000 % m) % m
	return Duration((r + t.nsec) % m)
}

// String returns the time formatted using the format string
//
//	"2006-01-02 15:04:05.999999999 -0700 MST"
func (t Time) String() string {
	s := t.Format("2006-01-02 15:04:05.999999999 -0700 MST")
	if t.mono != 0 {
		m := t.mono
		sign := byte('+')
		if m < 0 {
			sign = '-'
			m = -m
		}
		s += " m=" + string(rune(sign)) + decimal(uint64(m), 9)
	}
	return s
}

const (
	Layout      = "01/02 03:04:05PM '06 -0700"
	ANSIC       = "Mon Jan _2 15:04:05 2006"
	UnixDate    = "Mon Jan _2 15:04:05 MST 2006"
	RubyDate    = "Mon Jan 02 15:04:05 -0700 2006"
	RFC822      = "02 Jan 06 15:04 MST"
	RFC822Z     = "02 Jan 06 15:04 -0700"
	RFC850      = "Monday, 02-Jan-06 15:04:05 MST"
	RFC1123     = "Mon, 02 Jan 2006 15:04:05 MST"
	RFC1123Z    = "Mon, 02 Jan 2006 15:04:05 -0700"
	RFC3339     = "2006-01-02T15:04:05Z07:00"
	RFC3339Nano = "2006-01-02T15:04:05.999999999Z07:00"
	Kitchen     = "3:04PM"
	Stamp       = "Jan _2 15:04:05"
	StampMilli  = "Jan _2 15:04:05.000"
	StampMicro  = "Jan _2 15:04:05.000000"
	StampNano   = "Jan _2 15:04:05.000000000"
	DateTime    = "2006-01-02 15:04:05"
	DateOnly    = "2006-01-02"
	TimeOnly    = "15:04:05"
)

func hasPrefix(s string, p string) bool {
	return len(s) >= len(p) && s[:len(p)] == p
}

func pad(v int, width int, fill string) string {
	s := utoa(uint64(v))
	for len(s) < width {
		s = fill + s
	}
	return s
}

// A layout element, by kind.
const (
	stdNone = iota
	stdLongMonth
	stdMonth
	stdNumMonth
	stdZeroMonth
	stdLongWeekDay
	stdWeekDay
	stdDay
	stdUnderDay
	stdZeroDay
	stdUnderYearDay
	stdZeroYearDay
	stdHour
	stdHour12
	stdZeroHour12
	stdMinute
	stdZeroMinute
	stdSecond
	stdZeroSecond
	stdLongYear
	stdYear
	stdPM
	stdpm
	stdTZ
	stdISO8601TZ
	stdISO8601ColonTZ
	stdISO8601SecondsTZ
	stdISO8601ShortTZ
	stdISO8601ColonSecondsTZ
	stdNumTZ
	stdNumColonTZ
	stdNumSecondsTz
	stdNumShortTZ
	stdNumColonSecondsTZ
	stdFracSecond0
	stdFracSecond9
)

// nextStdChunk finds the first layout element in layout: the literal text
// before it, its kind, its length, and for a fractional second its digit count.
func nextStdChunk(layout string) (prefix string, std int, length int, digits int) {
	for i := 0; i < len(layout); i++ {
		rest := layout[i:]
		switch layout[i] {
		case 'J':
			if hasPrefix(rest, "January") {
				return layout[:i], stdLongMonth, 7, 0
			}
			if hasPrefix(rest, "Jan") {
				return layout[:i], stdMonth, 3, 0
			}
		case 'M':
			if hasPrefix(rest, "Monday") {
				return layout[:i], stdLongWeekDay, 6, 0
			}
			if hasPrefix(rest, "Mon") {
				return layout[:i], stdWeekDay, 3, 0
			}
			if hasPrefix(rest, "MST") {
				return layout[:i], stdTZ, 3, 0
			}
		case '0':
			if len(rest) >= 2 && '1' <= rest[1] && rest[1] <= '6' {
				k := []int{stdZeroMonth, stdZeroDay, stdZeroHour12, stdZeroMinute, stdZeroSecond, stdYear}
				return layout[:i], k[rest[1]-'1'], 2, 0
			}
			if hasPrefix(rest, "002") {
				return layout[:i], stdZeroYearDay, 3, 0
			}
		case '1':
			if hasPrefix(rest, "15") {
				return layout[:i], stdHour, 2, 0
			}
			return layout[:i], stdNumMonth, 1, 0
		case '2':
			if hasPrefix(rest, "2006") {
				return layout[:i], stdLongYear, 4, 0
			}
			return layout[:i], stdDay, 1, 0
		case '_':
			if hasPrefix(rest, "_2") {
				if hasPrefix(rest, "_2006") {
					return layout[:i+1], stdLongYear, 4, 0
				}
				return layout[:i], stdUnderDay, 2, 0
			}
			if hasPrefix(rest, "__2") {
				return layout[:i], stdUnderYearDay, 3, 0
			}
		case '3':
			return layout[:i], stdHour12, 1, 0
		case '4':
			return layout[:i], stdMinute, 1, 0
		case '5':
			return layout[:i], stdSecond, 1, 0
		case 'P':
			if hasPrefix(rest, "PM") {
				return layout[:i], stdPM, 2, 0
			}
		case 'p':
			if hasPrefix(rest, "pm") {
				return layout[:i], stdpm, 2, 0
			}
		case '-':
			if hasPrefix(rest, "-070000") {
				return layout[:i], stdNumSecondsTz, 7, 0
			}
			if hasPrefix(rest, "-07:00:00") {
				return layout[:i], stdNumColonSecondsTZ, 9, 0
			}
			if hasPrefix(rest, "-0700") {
				return layout[:i], stdNumTZ, 5, 0
			}
			if hasPrefix(rest, "-07:00") {
				return layout[:i], stdNumColonTZ, 6, 0
			}
			if hasPrefix(rest, "-07") {
				return layout[:i], stdNumShortTZ, 3, 0
			}
		case 'Z':
			if hasPrefix(rest, "Z070000") {
				return layout[:i], stdISO8601SecondsTZ, 7, 0
			}
			if hasPrefix(rest, "Z07:00:00") {
				return layout[:i], stdISO8601ColonSecondsTZ, 9, 0
			}
			if hasPrefix(rest, "Z0700") {
				return layout[:i], stdISO8601TZ, 5, 0
			}
			if hasPrefix(rest, "Z07:00") {
				return layout[:i], stdISO8601ColonTZ, 6, 0
			}
			if hasPrefix(rest, "Z07") {
				return layout[:i], stdISO8601ShortTZ, 3, 0
			}
		case '.', ',':
			if i+1 < len(layout) && (layout[i+1] == '0' || layout[i+1] == '9') {
				ch := layout[i+1]
				j := i + 1
				for j < len(layout) && layout[j] == ch {
					j++
				}
				if !(j < len(layout) && '0' <= layout[j] && layout[j] <= '9') {
					kind := stdFracSecond0
					if ch == '9' {
						kind = stdFracSecond9
					}
					return layout[:i], kind, j - i, j - (i + 1)
				}
			}
		}
	}
	return layout, stdNone, 0, 0
}

func zoneOffset(offset int, colon bool, seconds bool, short bool) string {
	sign := "+"
	if offset < 0 {
		sign = "-"
		offset = -offset
	}
	h := offset / 3600
	m := offset % 3600 / 60
	s := offset % 60
	out := sign + pad(h, 2, "0")
	if short {
		return out
	}
	if colon {
		out += ":"
	}
	out += pad(m, 2, "0")
	if seconds {
		if colon {
			out += ":"
		}
		out += pad(s, 2, "0")
	}
	return out
}

// Format returns a textual representation of the time value formatted
// according to the layout defined by the argument.
func (t Time) Format(layout string) string {
	year, month, day := t.Date()
	hour, min, sec := t.Clock()
	name, offset := t.Zone()
	yday := t.YearDay()
	out := ""
	for layout != "" {
		prefix, std, n, digits := nextStdChunk(layout)
		out += prefix
		if std == stdNone {
			break
		}
		layout = layout[len(prefix)+n:]
		switch std {
		case stdYear:
			y := year
			if y < 0 {
				y = -y
			}
			out += pad(y%100, 2, "0")
		case stdLongYear:
			out += pad(year, 4, "0")
		case stdMonth:
			out += shortMonthNames[month-1]
		case stdLongMonth:
			out += longMonthNames[month-1]
		case stdNumMonth:
			out += pad(int(month), 1, "0")
		case stdZeroMonth:
			out += pad(int(month), 2, "0")
		case stdWeekDay:
			out += shortDayNames[t.Weekday()]
		case stdLongWeekDay:
			out += longDayNames[t.Weekday()]
		case stdDay:
			out += pad(day, 1, "0")
		case stdUnderDay:
			out += pad(day, 2, " ")
		case stdZeroDay:
			out += pad(day, 2, "0")
		case stdUnderYearDay:
			out += pad(yday, 3, " ")
		case stdZeroYearDay:
			out += pad(yday, 3, "0")
		case stdHour:
			out += pad(hour, 2, "0")
		case stdHour12:
			hr := hour % 12
			if hr == 0 {
				hr = 12
			}
			out += pad(hr, 1, "0")
		case stdZeroHour12:
			hr := hour % 12
			if hr == 0 {
				hr = 12
			}
			out += pad(hr, 2, "0")
		case stdMinute:
			out += pad(min, 1, "0")
		case stdZeroMinute:
			out += pad(min, 2, "0")
		case stdSecond:
			out += pad(sec, 1, "0")
		case stdZeroSecond:
			out += pad(sec, 2, "0")
		case stdPM:
			if hour >= 12 {
				out += "PM"
			} else {
				out += "AM"
			}
		case stdpm:
			if hour >= 12 {
				out += "pm"
			} else {
				out += "am"
			}
		case stdISO8601TZ, stdISO8601ColonTZ, stdISO8601SecondsTZ, stdISO8601ShortTZ, stdISO8601ColonSecondsTZ:
			if offset == 0 {
				out += "Z"
			} else {
				out += zoneOffset(offset, std == stdISO8601ColonTZ || std == stdISO8601ColonSecondsTZ,
					std == stdISO8601SecondsTZ || std == stdISO8601ColonSecondsTZ, std == stdISO8601ShortTZ)
			}
		case stdNumTZ, stdNumColonTZ, stdNumSecondsTz, stdNumShortTZ, stdNumColonSecondsTZ:
			out += zoneOffset(offset, std == stdNumColonTZ || std == stdNumColonSecondsTZ,
				std == stdNumSecondsTz || std == stdNumColonSecondsTZ, std == stdNumShortTZ)
		case stdTZ:
			if name != "" {
				out += name
			} else {
				out += zoneOffset(offset, false, false, offset%3600 == 0)
			}
		case stdFracSecond0, stdFracSecond9:
			frac := pad(int(t.nsec), 9, "0")
			if std == stdFracSecond0 {
				out += "." + frac[:digits]
			} else {
				frac = frac[:digits]
				end := len(frac)
				for end > 0 && frac[end-1] == '0' {
					end--
				}
				if end > 0 {
					out += "." + frac[:end]
				}
			}
		}
	}
	return out
}

// A ParseError describes a problem parsing a time string.
type ParseError struct {
	Layout     string
	Value      string
	LayoutElem string
	ValueElem  string
	Message    string
}

// Error returns the string representation of a ParseError.
func (e *ParseError) Error() string {
	if e.Message == "" {
		return "parsing time " + quote(e.Value) + " as " + quote(e.Layout) + ": cannot parse " +
			quote(e.ValueElem) + " as " + quote(e.LayoutElem)
	}
	return "parsing time " + quote(e.Value) + e.Message
}

// getnum parses a number of up to two digits (fixed means exactly two).
func getnum(s string, fixed bool) (int, string, bool) {
	if len(s) == 0 || s[0] < '0' || s[0] > '9' {
		return 0, s, false
	}
	if len(s) == 1 || s[1] < '0' || s[1] > '9' {
		if fixed {
			return 0, s, false
		}
		return int(s[0] - '0'), s[1:], true
	}
	return int(s[0]-'0')*10 + int(s[1]-'0'), s[2:], true
}

func lookup(tab []string, val string) (int, string, bool) {
	for i, v := range tab {
		if len(val) >= len(v) && val[:len(v)] == v {
			return i, val[len(v):], true
		}
	}
	return -1, val, false
}

// Parse parses a formatted string and returns the time value it represents.
// Without zone information the time is in UTC.
func Parse(layout, value string) (Time, error) {
	alayout, avalue := layout, value
	year := 0
	month := -1
	day := -1
	yday := -1
	hour := 0
	min := 0
	sec := 0
	nsec := 0
	pmSet := false
	amSet := false
	zoneOff := -1
	zoneName := ""
	hasZone := false
	zoneOffset := 0
	for {
		prefix, std, n, digits := nextStdChunk(layout)
		stdstr := layout[len(prefix) : len(prefix)+n]
		if len(value) < len(prefix) || value[:len(prefix)] != prefix {
			return Time{}, &ParseError{alayout, avalue, prefix, value, ""}
		}
		value = value[len(prefix):]
		if std == stdNone {
			if len(value) != 0 {
				return Time{}, &ParseError{alayout, avalue, "", value, ": extra text: " + quote(value)}
			}
			break
		}
		layout = layout[len(prefix)+n:]
		hold := value
		ok := true
		rangeErr := ""
		switch std {
		case stdYear:
			if len(value) < 2 {
				ok = false
				break
			}
			var yy int
			yy, value, ok = getnum(value, true)
			if ok {
				if yy >= 69 {
					year = 1900 + yy
				} else {
					year = 2000 + yy
				}
			}
		case stdLongYear:
			if len(value) < 4 || value[0] < '0' || value[0] > '9' {
				ok = false
				break
			}
			year = 0
			for i := 0; i < 4; i++ {
				if value[i] < '0' || value[i] > '9' {
					ok = false
					break
				}
				year = year*10 + int(value[i]-'0')
			}
			value = value[4:]
		case stdMonth:
			month, value, ok = lookup(shortMonthNames, value)
			month++
		case stdLongMonth:
			month, value, ok = lookup(longMonthNames, value)
			month++
		case stdNumMonth, stdZeroMonth:
			month, value, ok = getnum(value, std == stdZeroMonth)
			if ok && (month <= 0 || 12 < month) {
				rangeErr = "month"
			}
		case stdWeekDay:
			_, value, ok = lookup(shortDayNames, value)
		case stdLongWeekDay:
			_, value, ok = lookup(longDayNames, value)
		case stdDay, stdUnderDay, stdZeroDay:
			if std == stdUnderDay && len(value) > 0 && value[0] == ' ' {
				value = value[1:]
			}
			day, value, ok = getnum(value, std == stdZeroDay)
		case stdUnderYearDay, stdZeroYearDay:
			for i := 0; i < 2; i++ {
				if std == stdUnderYearDay && len(value) > 0 && value[0] == ' ' {
					value = value[1:]
				}
			}
			if len(value) < 3 {
				ok = false
				break
			}
			yday = 0
			for i := 0; i < 3; i++ {
				if value[i] < '0' || value[i] > '9' {
					ok = false
					break
				}
				yday = yday*10 + int(value[i]-'0')
			}
			value = value[3:]
		case stdHour:
			hour, value, ok = getnum(value, false)
			if ok && (hour < 0 || 24 <= hour) {
				rangeErr = "hour"
			}
		case stdHour12, stdZeroHour12:
			hour, value, ok = getnum(value, std == stdZeroHour12)
			if ok && (hour < 0 || 12 < hour) {
				rangeErr = "hour"
			}
		case stdMinute, stdZeroMinute:
			min, value, ok = getnum(value, std == stdZeroMinute)
			if ok && (min < 0 || 60 <= min) {
				rangeErr = "minute"
			}
		case stdSecond, stdZeroSecond:
			sec, value, ok = getnum(value, std == stdZeroSecond)
			if ok && (sec < 0 || 60 <= sec) {
				rangeErr = "second"
				break
			}
			// A fractional second in the value is accepted when the layout
			// does not ask for one next.
			if len(value) >= 2 && (value[0] == '.' || value[0] == ',') && '0' <= value[1] && value[1] <= '9' {
				_, nstd, _, _ := nextStdChunk(layout)
				if nstd == stdFracSecond0 || nstd == stdFracSecond9 {
					break
				}
				j := 1
				for j < len(value) && '0' <= value[j] && value[j] <= '9' {
					j++
				}
				nsec = parseNanos(value[1:j])
				value = value[j:]
			}
		case stdPM:
			if len(value) < 2 {
				ok = false
				break
			}
			switch value[:2] {
			case "PM":
				pmSet = true
			case "AM":
				amSet = true
			default:
				ok = false
			}
			value = value[2:]
		case stdpm:
			if len(value) < 2 {
				ok = false
				break
			}
			switch value[:2] {
			case "pm":
				pmSet = true
			case "am":
				amSet = true
			default:
				ok = false
			}
			value = value[2:]
		case stdISO8601TZ, stdISO8601ColonTZ, stdISO8601SecondsTZ, stdISO8601ShortTZ, stdISO8601ColonSecondsTZ,
			stdNumTZ, stdNumShortTZ, stdNumColonTZ, stdNumSecondsTz, stdNumColonSecondsTZ:
			if (std == stdISO8601TZ || std == stdISO8601ShortTZ || std == stdISO8601ColonTZ ||
				std == stdISO8601SecondsTZ || std == stdISO8601ColonSecondsTZ) && len(value) >= 1 && value[0] == 'Z' {
				value = value[1:]
				hasZone = true
				zoneOff = 0
				zoneName = "UTC"
				break
			}
			if len(value) < 3 || (value[0] != '+' && value[0] != '-') {
				ok = false
				break
			}
			sign := 1
			if value[0] == '-' {
				sign = -1
			}
			hh, rest, ok1 := getnum(value[1:], true)
			mm, ss := 0, 0
			ok2, ok3 := true, true
			if std != stdNumShortTZ && std != stdISO8601ShortTZ {
				if std == stdNumColonTZ || std == stdISO8601ColonTZ || std == stdNumColonSecondsTZ || std == stdISO8601ColonSecondsTZ {
					if len(rest) == 0 || rest[0] != ':' {
						ok = false
						break
					}
					rest = rest[1:]
				}
				mm, rest, ok2 = getnum(rest, true)
				if std == stdNumSecondsTz || std == stdNumColonSecondsTZ || std == stdISO8601SecondsTZ || std == stdISO8601ColonSecondsTZ {
					if std == stdNumColonSecondsTZ || std == stdISO8601ColonSecondsTZ {
						if len(rest) == 0 || rest[0] != ':' {
							ok = false
							break
						}
						rest = rest[1:]
					}
					ss, rest, ok3 = getnum(rest, true)
				}
			}
			if !ok1 || !ok2 || !ok3 {
				ok = false
				break
			}
			value = rest
			hasZone = true
			zoneOff = sign * (hh*3600 + mm*60 + ss)
			zoneName = ""
		case stdTZ:
			if len(value) >= 3 && value[:3] == "UTC" {
				value = value[3:]
				hasZone = true
				zoneOff = 0
				zoneName = "UTC"
				break
			}
			j := 0
			for j < len(value) && 'A' <= value[j] && value[j] <= 'Z' {
				j++
			}
			if j < 3 {
				ok = false
				break
			}
			zoneName = value[:j]
			value = value[j:]
			hasZone = true
			zoneOff = 0
		case stdFracSecond0:
			if len(value) < digits+1 || (value[0] != '.' && value[0] != ',') {
				ok = false
				break
			}
			for i := 1; i <= digits; i++ {
				if value[i] < '0' || value[i] > '9' {
					ok = false
					break
				}
			}
			if ok {
				nsec = parseNanos(value[1 : digits+1])
				value = value[digits+1:]
			}
		case stdFracSecond9:
			if len(value) < 2 || (value[0] != '.' && value[0] != ',') || value[1] < '0' || value[1] > '9' {
				break
			}
			j := 1
			for j < len(value) && '0' <= value[j] && value[j] <= '9' {
				j++
			}
			nsec = parseNanos(value[1:j])
			value = value[j:]
		}
		if rangeErr != "" {
			return Time{}, &ParseError{alayout, avalue, stdstr, value, ": " + rangeErr + " out of range"}
		}
		if !ok {
			return Time{}, &ParseError{alayout, avalue, stdstr, hold, ""}
		}
	}
	if pmSet && hour < 12 {
		hour += 12
	} else if amSet && hour == 12 {
		hour = 0
	}
	if yday >= 0 {
		if month < 0 {
			base := Date(year, January, 1, 0, 0, 0, 0, UTC).AddDate(0, 0, yday-1)
			m, d := int(base.Month()), base.Day()
			month, day = m, d
		}
	}
	if month < 0 {
		month = 1
	}
	if day < 0 {
		day = 1
	}
	// Validate the day of the month.
	if day < 1 || day > daysIn(Month(month), year) {
		return Time{}, &ParseError{alayout, avalue, "", value, ": day out of range"}
	}
	loc := UTC
	if hasZone {
		if zoneOff == 0 && (zoneName == "UTC" || zoneName == "") {
			loc = UTC
			if zoneName == "" {
				loc = FixedZone("", 0)
			}
		} else {
			loc = FixedZone(zoneName, zoneOff)
		}
		zoneOffset = zoneOff
	}
	t := Date(year, Month(month), day, hour, min, sec, nsec, UTC)
	t.sec -= int64(zoneOffset)
	t.loc = loc
	return t, nil
}

func parseNanos(s string) int {
	ns := 0
	for i := 0; i < 9; i++ {
		ns *= 10
		if i < len(s) {
			ns += int(s[i] - '0')
		}
	}
	return ns
}

func daysIn(m Month, year int) int {
	switch m {
	case February:
		if year%4 == 0 && (year%100 != 0 || year%400 == 0) {
			return 29
		}
		return 28
	case April, June, September, November:
		return 30
	}
	return 31
}

// sleepers holds the deadline of every goroutine in Sleep, in arrival order.
var sleepers []int64

// Sleep pauses the current goroutine for at least the duration d. A negative
// or zero duration causes Sleep to return immediately.
func Sleep(d Duration) {
	if d <= 0 {
		return
	}
	deadline := monoNano() + int64(d)
	sleepers = append(sleepers, deadline)
	for {
		now := monoNano()
		if now >= deadline && earliest(deadline) {
			break
		}
		wait := deadline - now
		if wait > 1000000 {
			wait = 1000000
		}
		if wait > 0 {
			sleepHint(wait)
		}
		yield()
	}
	for i, s := range sleepers {
		if s == deadline {
			sleepers = append(sleepers[:i], sleepers[i+1:]...)
			break
		}
	}
}

// earliest reports whether no sleeper registered before this one, and not
// later than it, is still waiting: sleepers wake in deadline order.
func earliest(deadline int64) bool {
	for _, s := range sleepers {
		if s < deadline {
			return false
		}
		if s == deadline {
			return true
		}
	}
	return true
}

// The Timer type represents a single event. When the Timer expires, the
// current time will be sent on C, unless the Timer was created by AfterFunc.
type Timer struct {
	C       <-chan Time
	c       chan Time
	f       func()
	gen     int
	pending bool
}

func (t *Timer) start(d Duration) {
	t.gen++
	gen := t.gen
	t.pending = true
	go func() {
		Sleep(d)
		if t.gen != gen || !t.pending {
			return
		}
		t.pending = false
		if t.f != nil {
			t.f()
			return
		}
		select {
		case t.c <- Now():
		default:
		}
	}()
}

// NewTimer creates a new Timer that will send the current time on its channel
// after at least duration d.
func NewTimer(d Duration) *Timer {
	c := make(chan Time, 1)
	t := &Timer{C: c, c: c}
	t.start(d)
	return t
}

// AfterFunc waits for the duration to elapse and then calls f in its own
// goroutine. It returns a Timer that can be used to cancel the call.
func AfterFunc(d Duration, f func()) *Timer {
	t := &Timer{f: f}
	t.start(d)
	return t
}

// Stop prevents the Timer from firing. It returns true if the call stops the
// timer, false if the timer has already expired or been stopped.
func (t *Timer) Stop() bool {
	was := t.pending
	t.pending = false
	t.gen++
	return was
}

// Reset changes the timer to expire after duration d. It returns true if the
// timer had been active, false if the timer had expired or been stopped.
func (t *Timer) Reset(d Duration) bool {
	was := t.pending
	t.start(d)
	return was
}

// After waits for the duration to elapse and then sends the current time on
// the returned channel.
func After(d Duration) <-chan Time {
	return NewTimer(d).C
}

// A Ticker holds a channel that delivers “ticks” of a clock at intervals.
type Ticker struct {
	C       <-chan Time
	c       chan Time
	stopped bool
	gen     int
}

// NewTicker returns a new Ticker containing a channel that will send the
// current time on the channel after each tick. The period of the ticks is
// specified by the duration argument.
func NewTicker(d Duration) *Ticker {
	if d <= 0 {
		panic("non-positive interval for NewTicker")
	}
	c := make(chan Time, 1)
	t := &Ticker{C: c, c: c}
	t.run(d)
	return t
}

func (t *Ticker) run(d Duration) {
	t.gen++
	gen := t.gen
	go func() {
		next := monoNano() + int64(d)
		for {
			for monoNano() < next {
				Sleep(Duration(next - monoNano()))
			}
			if t.stopped || t.gen != gen {
				return
			}
			select {
			case t.c <- Now():
			default:
			}
			next += int64(d)
		}
	}()
}

// Stop turns off a ticker. After Stop, no more ticks will be sent.
func (t *Ticker) Stop() { t.stopped = true }

// Reset stops a ticker and resets its period to the specified duration.
func (t *Ticker) Reset(d Duration) {
	if d <= 0 {
		panic("non-positive interval for Ticker.Reset")
	}
	t.stopped = false
	t.run(d)
}

// Tick is a convenience wrapper for NewTicker providing access to the ticking
// channel only.
func Tick(d Duration) <-chan Time {
	if d <= 0 {
		return nil
	}
	return NewTicker(d).C
}
