CREATE TABLE IF NOT EXISTS `Items` (
	`id` integer primary key NOT NULL UNIQUE,
	`name` TEXT NOT NULL,
	`price_per_unit` REAL NOT NULL,
	`unit` TEXT,
	`age_limit` INTEGER
);
CREATE TABLE IF NOT EXISTS `Shops` (
	`id` integer primary key NOT NULL UNIQUE,
	`name` TEXT NOT NULL,
	`address` TEXT NOT NULL UNIQUE
);
CREATE TABLE IF NOT EXISTS `Shop_stock` (
	`shop_id` INTEGER NOT NULL,
	`item_id` INTEGER NOT NULL,
	`stock` INTEGER,
FOREIGN KEY(`shop_id`) REFERENCES `Shops`(`id`),
FOREIGN KEY(`item_id`) REFERENCES `Items`(`id`)
);
CREATE TABLE IF NOT EXISTS `Users` (
	`username` TEXT NOT NULL UNIQUE,
	`password` TEXT NOT NULL,
	`salt` TEXT NOT NULL UNIQUE,
	`first_name` TEXT NOT NULL,
	`last_name` TEXT NOT NULL,
	`dob` REAL NOT NULL,
	`email` TEXT NOT NULL,
	`card_num` TEXT,
	`exp` REAL,
	`cvv` TEXT
);
CREATE TABLE IF NOT EXISTS `Transactions` (
	`id` INTEGER NOT NULL,
	`username` INTEGER NOT NULL,
	`shop_id` INTEGER NOT NULL,
	`time` REAL NOT NULL,
FOREIGN KEY(`username`) REFERENCES `Users`(`username`),
FOREIGN KEY(`shop_id`) REFERENCES `Shops`(`id`)
);
CREATE TABLE IF NOT EXISTS `Purchase_records` (
	`transaction_id` INTEGER NOT NULL,
	`item_id` INTEGER NOT NULL,
	`quantity` REAL NOT NULL,
FOREIGN KEY(`transaction_id`) REFERENCES `Transactions`(`id`),
FOREIGN KEY(`item_id`) REFERENCES `Items`(`id`)
);