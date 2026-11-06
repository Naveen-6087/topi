# One mixnet node: an Ubuntu 24.04 instance in the region of the calling provider, with its own key pair
# and security group (mixnet ports open to the internet; SSH and stats only to the operator's address).
terraform {
  required_providers {
    aws = { source = "hashicorp/aws" }
  }
}

variable "name" { type = string }
variable "role" { type = string }
variable "ports" { type = list(number) }
variable "operator_cidr" { type = string }
variable "ssh_public_key" { type = string }
variable "instance_type" { type = string }

data "aws_region" "this" {}

data "aws_ami" "ubuntu" {
  most_recent = true
  owners      = ["099720109477"] # Canonical
  filter {
    name   = "name"
    values = ["ubuntu/images/hvm-ssd-gp3/ubuntu-noble-24.04-amd64-server-*"]
  }
}

resource "aws_key_pair" "node" {
  key_name   = "${var.name}-${var.role}"
  public_key = var.ssh_public_key
  tags       = { Project = var.name }
}

resource "aws_security_group" "node" {
  name        = "${var.name}-${var.role}"
  description = "Mixnet ${var.role}"
  tags        = { Project = var.name }

  dynamic "ingress" {
    for_each = var.ports
    content {
      description = "mixnet"
      from_port   = ingress.value
      to_port     = ingress.value
      protocol    = "tcp"
      cidr_blocks = ["0.0.0.0/0"]
    }
  }
  ingress {
    description = "ssh from the operator"
    from_port   = 22
    to_port     = 22
    protocol    = "tcp"
    cidr_blocks = [var.operator_cidr]
  }
  ingress {
    description = "node stats (JSON)"
    from_port   = 47300
    to_port     = 47300
    protocol    = "tcp"
    cidr_blocks = [var.operator_cidr]
  }
  ingress {
    description = "Prometheus metrics"
    from_port   = 47400
    to_port     = 47400
    protocol    = "tcp"
    cidr_blocks = [var.operator_cidr]
  }
  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
}

resource "aws_instance" "node" {
  ami                    = data.aws_ami.ubuntu.id
  instance_type          = var.instance_type
  key_name               = aws_key_pair.node.key_name
  vpc_security_group_ids = [aws_security_group.node.id]

  # No burst billing: t3 defaults to "unlimited" CPU credits, which can charge beyond the baseline.
  credit_specification {
    cpu_credits = "standard"
  }
  root_block_device {
    volume_size           = 8
    volume_type           = "gp3"
    delete_on_termination = true
  }
  metadata_options {
    http_tokens = "required"
  }
  tags = { Name = "${var.name}-${var.role}", Project = var.name, Role = var.role }
}

output "node" {
  value = {
    role        = var.role
    region      = data.aws_region.this.region
    instance_id = aws_instance.node.id
    public_ip   = aws_instance.node.public_ip
    type        = var.instance_type
  }
}
