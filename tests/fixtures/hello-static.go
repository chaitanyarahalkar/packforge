package main

import (
	"fmt"
	"os"
)

func checksum(text string) uint32 {
	value := uint32(2166136261)
	for index := 0; index < len(text); index++ {
		value ^= uint32(text[index])
		value *= 16777619
	}
	return value
}

func main() {
	environment, present := os.LookupEnv("PACKFORGE_SMOKE")
	if len(os.Args) != 2 || !present {
		fmt.Fprintln(os.Stderr, "expected one argument and PACKFORGE_SMOKE")
		os.Exit(2)
	}

	argument := os.Args[1]
	fmt.Printf("packforge-smoke argc=%d arg=%s env=%s checksum=%d\n",
		len(os.Args), argument, environment, checksum(argument)^checksum(environment))
	if argument != "round-trip" {
		os.Exit(3)
	}
}
