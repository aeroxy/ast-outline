package com.example;

import com.example.util.Formatter;

public class Greeter {
    public String greet(String name) {
        return new Formatter().format("hello " + name);
    }
}
