MsgPackNet - A networking layer based on MessagePack messages
=============================================================

[![Build Status](https://travis-ci.org/dswd/msgpacknet.rs.svg?branch=master)](https://travis-ci.org/dswd/msgpacknet.rs)
[![Coverage Status](https://coveralls.io/repos/dswd/msgpacknet.rs/badge.svg?branch=master&service=github)](https://coveralls.io/github/dswd/msgpacknet.rs?branch=master)
[![Latest Version](https://img.shields.io/crates/v/msgpacknet.svg)](https://crates.io/crates/msgpacknet)

This crate provides an abstraction layer above TCP that uses [MessagePack](http://www.msgpack.org) encoded messages instead of pure byte streams.
It also abstracts from addresses and connections and instead uses node identifiers to distinguish and address nodes.

[Documentation](http://dswd.github.io/msgpacknet.rs/msgpacknet/index.html)
