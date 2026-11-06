# The mixnet on AWS: four nodes in four regions, so no single jurisdiction or data centre sees a packet's
# whole path. Gateway in Mumbai, mix in Frankfurt, exit in N. Virginia, reply relay in Singapore.
# Driven by deploy.sh (up / provision / down).
terraform {
  required_version = ">= 1.6"
  required_providers {
    aws = { source = "hashicorp/aws", version = "~> 6.0" }
  }
}

variable "name" {
  description = "Deployment name: prefixes resource names and tags every resource (Project = name)"
  type        = string
  default     = "topi"
}
variable "operator_cidr" {
  description = "Source allowed to SSH and read node stats (x.x.x.x/32, or 0.0.0.0/0)"
  type        = string
}
variable "ssh_public_key" { type = string }
variable "instance_type" {
  type    = string
  default = "t3.micro"
}

locals {
  tags = { Project = var.name, ManagedBy = "terraform" }
}

provider "aws" {
  alias  = "mumbai"
  region = "ap-south-1"
  default_tags { tags = local.tags }
}
provider "aws" {
  alias  = "frankfurt"
  region = "eu-central-1"
  default_tags { tags = local.tags }
}
provider "aws" {
  alias  = "virginia"
  region = "us-east-1"
  default_tags { tags = local.tags }
}
provider "aws" {
  alias  = "singapore"
  region = "ap-southeast-1"
  default_tags { tags = local.tags }
}

module "gateway" {
  source         = "./node"
  providers      = { aws = aws.mumbai }
  name           = var.name
  role           = "gateway"
  ports          = [47101, 47201, 47250] # relay, rate limiter, reply store
  operator_cidr  = var.operator_cidr
  ssh_public_key = var.ssh_public_key
  instance_type  = var.instance_type
}

module "mix" {
  source         = "./node"
  providers      = { aws = aws.frankfurt }
  name           = var.name
  role           = "mix"
  ports          = [47102]
  operator_cidr  = var.operator_cidr
  ssh_public_key = var.ssh_public_key
  instance_type  = var.instance_type
}

module "exit" {
  source         = "./node"
  providers      = { aws = aws.virginia }
  name           = var.name
  role           = "exit"
  ports          = [47103]
  operator_cidr  = var.operator_cidr
  ssh_public_key = var.ssh_public_key
  instance_type  = var.instance_type
}

module "reply_relay" {
  source         = "./node"
  providers      = { aws = aws.singapore }
  name           = var.name
  role           = "reply-relay"
  ports          = [47104]
  operator_cidr  = var.operator_cidr
  ssh_public_key = var.ssh_public_key
  instance_type  = var.instance_type
}

output "nodes" {
  value = [module.gateway.node, module.mix.node, module.exit.node, module.reply_relay.node]
}
