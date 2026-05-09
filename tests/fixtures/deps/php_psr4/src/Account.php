<?php

namespace App;

class Account
{
    public string $email;

    public function __construct(string $email)
    {
        $this->email = $email;
    }
}
