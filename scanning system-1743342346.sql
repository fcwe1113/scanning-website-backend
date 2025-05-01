-- Users definition

CREATE TABLE Users (
                       username TEXT NOT NULL,
                       password TEXT NOT NULL,
                       first_name TEXT NOT NULL,
                       last_name TEXT NOT NULL,
                       dob TEXT NOT NULL,
                       email TEXT NOT NULL,
                       card_num TEXT,
                       "exp" TEXT,
                       cvv TEXT,
                       CONSTRAINT Users_PK PRIMARY KEY (username)
);

-- Shops definition

CREATE TABLE `Shops` (
                         `id` integer primary key NOT NULL UNIQUE,
                         `name` TEXT NOT NULL,
                         `address` TEXT NOT NULL UNIQUE
);

-- Items definition

CREATE TABLE `Items` (
                         `id` integer primary key NOT NULL UNIQUE,
                         `name` TEXT NOT NULL,
                         `price_per_unit` INTEGER,
                         `unit` TEXT,
                         `age_limit` INTEGER
);

-- Transactions definition

CREATE TABLE Transactions (
                              id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
                              username TEXT NOT NULL,
                              shop_id INTEGER NOT NULL,
                              time TEXT NOT NULL,
                              CONSTRAINT Transactions_Shops_FK FOREIGN KEY (shop_id) REFERENCES Shops(id),
                              CONSTRAINT Transactions_Users_FK FOREIGN KEY (username) REFERENCES Users(username)
);

-- Purchase_records definition

CREATE TABLE Purchase_records (
                                  transaction_id INTEGER NOT NULL,
                                  item_id INTEGER NOT NULL,
                                  quantity REAL NOT NULL,
                                  CONSTRAINT Purchase_records_PK PRIMARY KEY (transaction_id,item_id),
                                  CONSTRAINT Purchase_records_Items_FK FOREIGN KEY (item_id) REFERENCES Items(id),
                                  CONSTRAINT Purchase_records_Transactions_FK FOREIGN KEY (transaction_id) REFERENCES Transactions(id)
);

-- Shop_stock definition

CREATE TABLE `Shop_stock` (
                              `shop_id` INTEGER NOT NULL,
                              `item_id` INTEGER NOT NULL,
                              `stock` INTEGER,
                              FOREIGN KEY(`shop_id`) REFERENCES `Shops`(`id`),
                              FOREIGN KEY(`item_id`) REFERENCES `Items`(`id`)
);