-- Initial database setup for development
-- This script runs when the PostgreSQL container first starts

-- Enable UUID extension for generating UUIDs
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- Create the main database (already created by POSTGRES_DB env var)
-- Just ensuring we're connected to the right database
\c preconfirmation_gateway;

-- Create a test database for running tests
CREATE DATABASE preconfirmation_gateway_test;

-- Grant permissions
GRANT ALL PRIVILEGES ON DATABASE preconfirmation_gateway TO postgres;
GRANT ALL PRIVILEGES ON DATABASE preconfirmation_gateway_test TO postgres;