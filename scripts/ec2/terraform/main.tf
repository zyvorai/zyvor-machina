# Terraform (hashicorp/aws) against Machina's EC2-compatible endpoints. Throwaway resources only, all named var.name.
# Driven by scripts/ec2/run-compat.sh; see README.md. Not every resource is expected to apply: docs/cloud-ec2-clients.md.

terraform {
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
}

provider "aws" {
  region     = "machina"
  access_key = var.access_key
  secret_key = var.secret_key
  insecure   = var.insecure

  skip_credentials_validation = true
  skip_requesting_account_id  = true
  skip_metadata_api_check     = true
  skip_region_validation      = true

  endpoints {
    ec2         = "${var.endpoint}/ec2"
    autoscaling = "${var.endpoint}/autoscaling"
    elbv2       = "${var.endpoint}/elbv2"
    elb         = "${var.endpoint}/elbv2"
    cloudwatch  = "${var.endpoint}/monitoring"
  }
}

locals {
  own_vpc    = var.vpc_id == ""
  own_subnet = var.subnet_id == ""
  vpc_id     = local.own_vpc ? aws_vpc.this[0].id : var.vpc_id
  subnet_id  = local.own_subnet ? aws_subnet.this[0].id : var.subnet_id
}

resource "aws_vpc" "this" {
  count      = local.own_vpc ? 1 : 0
  cidr_block = var.vpc_cidr
  tags       = { Name = var.name }
}

resource "aws_subnet" "this" {
  count             = local.own_subnet ? 1 : 0
  vpc_id            = local.vpc_id
  cidr_block        = var.subnet_cidr
  availability_zone = var.availability_zone
  tags              = { Name = var.name }
}

resource "aws_internet_gateway" "this" {
  count  = var.create_igw ? 1 : 0
  vpc_id = local.vpc_id
  tags   = { Name = var.name }
}

resource "aws_route_table" "this" {
  vpc_id = local.vpc_id
  tags   = { Name = var.name }

  dynamic "route" {
    for_each = var.create_igw ? [1] : []
    content {
      cidr_block = "0.0.0.0/0"
      gateway_id = aws_internet_gateway.this[0].id
    }
  }
}

resource "aws_route_table_association" "this" {
  subnet_id      = local.subnet_id
  route_table_id = aws_route_table.this.id
}

resource "aws_security_group" "this" {
  name        = var.name
  description = "machina compat"
  vpc_id      = local.vpc_id

  ingress {
    from_port   = 22
    to_port     = 22
    protocol    = "tcp"
    cidr_blocks = ["10.0.0.0/8"]
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  tags = { Name = var.name }
}

resource "aws_instance" "this" {
  ami                    = var.ami
  instance_type          = var.instance_type
  subnet_id              = local.subnet_id
  vpc_security_group_ids = [aws_security_group.this.id]
  monitoring             = false
  tags                   = { Name = var.name }
}

resource "aws_ebs_volume" "this" {
  availability_zone = var.availability_zone
  size              = 1
  tags              = { Name = var.name }
}

resource "aws_volume_attachment" "this" {
  device_name = "/dev/vdb"
  volume_id   = aws_ebs_volume.this.id
  instance_id = aws_instance.this.id
}

resource "aws_launch_template" "this" {
  count         = var.try_launch_template ? 1 : 0
  name          = var.name
  image_id      = var.ami
  instance_type = var.instance_type
}

resource "aws_launch_configuration" "this" {
  count             = var.try_launch_template ? 0 : 1
  name              = var.name
  image_id          = var.ami
  instance_type     = var.instance_type
  enable_monitoring = false
}

# min 0 / desired 0: the group exists, nothing is launched.
resource "aws_autoscaling_group" "this" {
  name                 = var.name
  min_size             = 0
  max_size             = 1
  desired_capacity     = 0
  vpc_zone_identifier  = [local.subnet_id]
  launch_configuration = var.try_launch_template ? null : aws_launch_configuration.this[0].name
  force_delete         = true

  dynamic "launch_template" {
    for_each = var.try_launch_template ? [1] : []
    content {
      id      = aws_launch_template.this[0].id
      version = "$Latest"
    }
  }

  tag {
    key                 = "Name"
    value               = var.name
    propagate_at_launch = true
  }
}

resource "aws_lb" "this" {
  name               = var.name
  load_balancer_type = "network"
  internal           = true
  subnets            = [local.subnet_id]
}

resource "aws_lb_target_group" "this" {
  name        = var.name
  port        = 18081
  protocol    = "TCP"
  target_type = "instance"
  vpc_id      = local.vpc_id
}

resource "aws_lb_listener" "this" {
  load_balancer_arn = aws_lb.this.arn
  port              = 18081
  protocol          = "TCP"

  default_action {
    type             = "forward"
    target_group_arn = aws_lb_target_group.this.arn
  }
}
