// The deterministic subset of `time`: Duration arithmetic and printing, Time
// values built with Date/Unix in UTC or a fixed zone, Format/Parse layouts,
// and Sleep/After/Tick/Timer ordering by deadline.
package main

import (
	"fmt"
	"time"
)

func main() {
	d := 90*time.Minute + 30*time.Second + 500*time.Millisecond
	fmt.Println(d, d.Hours(), d.Minutes(), d.Seconds(), d.Milliseconds())
	fmt.Println(time.Duration(1500)*time.Microsecond, time.Duration(0), 2*time.Nanosecond, 1500*time.Millisecond, -3*time.Second)
	fmt.Println(d.Truncate(time.Hour), d.Round(time.Hour), time.Duration(999))
	fmt.Printf("%v %d %s %T\n", time.Second, time.Second, time.Millisecond, time.Second)
	t := time.Date(2024, time.February, 29, 13, 4, 5, 123456789, time.UTC)
	fmt.Println(t)
	fmt.Println(t.Year(), t.Month(), t.Day(), t.Hour(), t.Minute(), t.Second(), t.Nanosecond(), t.Weekday(), t.YearDay())
	fmt.Println(t.Format(time.RFC3339), t.Format(time.RFC3339Nano), t.Format(time.RFC1123), t.Format(time.Kitchen))
	fmt.Println(t.Format("Mon Jan _2 15:04:05 2006"), t.Format("02/01/06 03:04PM"), t.Format("2006-01-02T15:04:05.000Z07:00"))
	fmt.Println(t.Unix(), t.UnixNano(), t.UnixMilli())
	u := t.Add(36 * time.Hour)
	fmt.Println(u, u.Sub(t), u.After(t), t.Before(u), t.Equal(t))
	fmt.Println(t.AddDate(0, 1, 1).Format(time.DateOnly), t.AddDate(1, 0, 0).Format(time.DateOnly))
	fmt.Println(time.Unix(0, 0).UTC(), time.Unix(1700000000, 0).UTC().Format(time.DateTime))
	var z time.Time
	fmt.Println(z.IsZero(), z, z.Unix())
	p, err := time.Parse("2006-01-02 15:04:05", "2023-07-04 09:08:07")
	fmt.Println(p, err)
	_, err = time.Parse("2006-01-02", "2023-13-04")
	fmt.Println(err)
	_, err = time.Parse("2006-01-02", "hello")
	fmt.Println(err)
	pd, err := time.ParseDuration("1h15m30.5s")
	fmt.Println(pd, err)
	_, err = time.ParseDuration("abc")
	fmt.Println(err)
	fmt.Println(t.Truncate(time.Hour).Format(time.TimeOnly), t.Round(time.Hour).Format(time.TimeOnly))
	fmt.Println(time.March, time.Saturday, time.Month(13))
	te := t.In(time.FixedZone("EST", -5*3600))
	fmt.Println(te, te.Format(time.RFC822Z))

	start := time.Now()
	time.Sleep(20 * time.Millisecond)
	el := time.Since(start)
	fmt.Println(el >= 20*time.Millisecond, el < 2*time.Second)
	done := make(chan string)
	go func() {
		time.Sleep(60 * time.Millisecond)
		done <- "slow"
	}()
	go func() {
		time.Sleep(10 * time.Millisecond)
		done <- "fast"
	}()
	fmt.Println(<-done, <-done)
	select {
	case <-time.After(10 * time.Millisecond):
		fmt.Println("timeout")
	case v := <-make(chan int):
		fmt.Println(v)
	}
	tk := time.NewTicker(5 * time.Millisecond)
	n := 0
	for range tk.C {
		n++
		if n == 3 {
			tk.Stop()
			break
		}
	}
	fmt.Println("ticks", n)
	tm := time.NewTimer(time.Hour)
	fmt.Println(tm.Stop(), tm.Stop())
	fired := make(chan bool)
	time.AfterFunc(5*time.Millisecond, func() { fired <- true })
	fmt.Println(<-fired)
}
