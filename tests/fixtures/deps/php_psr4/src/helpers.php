<?php

function format_email(string $email): string
{
    return strtolower(trim($email));
}
