# README

Help delete Mac system generated .DS_ Stroe file.

## INSTALL

If there is no rust environment, please install rust.

```shell
brew install rust
```

If there is a rust environment/installed

```shell
cargo install rm_ds_store
```

## USAGE

When there is no parameter execution, the path of the currently executed command will be the root path to traverse the
file and delete it. DS_Store file

```shell
  rmds
```

-p or --path parameter specifies the path

```shell
rmds -p ~/code
rmds --path ~/code
```

-s or --show parameter Define whether to display the deletion log,Default to true.
```shell
rmds -s false 
rmds --show false
```

## UNINSTALL
```shell
cargo unistall rm_ds_store
```
