package main

import "fmt"

type Account struct {
	name    string
	balance int
}

func (a Account) describe() string {
	return a.name
}

func main() {
	a := Account{name: "alice", balance: 100}
	b := a
	b.balance = 999
	fmt.Println(a.describe(), a.balance, b.balance)
	fmt.Println(a, b)

	accts := []Account{{name: "x", balance: 10}, {name: "y", balance: 20}}
	tot := 0
	for _, ac := range accts {
		tot += ac.balance
	}
	fmt.Println("total", tot)
}
