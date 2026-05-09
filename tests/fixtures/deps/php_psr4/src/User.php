<?php

namespace App;

use App\Account;

class User
{
    private Account $account;

    public function __construct(Account $account)
    {
        $this->account = $account;
    }

    public function helpers(): array
    {
        require_once 'helpers.php';
        return [];
    }

    public function moreHelpers(): array
    {
        // Parenthesized form — `require_once(<expr>)`. Tree-sitter-php
        // wraps the string in `parenthesized_expression`; the extractor
        // must descend into it to find the path.
        require_once('utils.php');
        return [];
    }
}
